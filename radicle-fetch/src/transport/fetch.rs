use std::{
    borrow::Cow,
    io::{self, BufRead},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use gix_features::progress::{prodash::progress, Progress};
use gix_hash::ObjectId;
use gix_pack as pack;
use gix_protocol::{
    fetch::{self, Delegate, DelegateBlocking},
    handshake::{self, Ref},
    ls_refs, FetchConnection,
};
use gix_transport::{bstr::BString, client, Protocol};

use super::{indicate_end_of_interaction, Connection};

pub type Error = gix_protocol::fetch::Error;

pub mod error {
    use std::io;

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum PackWriter {
        #[error(transparent)]
        Io(#[from] io::Error),

        #[error(transparent)]
        Write(#[from] gix_pack::bundle::write::Error),
    }
}

pub struct PackWriter {
    git_dir: PathBuf,
    interrupt: Arc<AtomicBool>,
    max_threads: Option<usize>,
}

impl PackWriter {
    pub fn write_pack(
        &self,
        pack: impl BufRead,
        progress: impl Progress,
    ) -> Result<pack::bundle::write::Outcome, error::PackWriter> {
        use gix_odb::FindExt as _;

        let options = pack::bundle::write::Options {
            thread_limit: self.max_threads,
            iteration_mode: pack::data::input::Mode::Verify,
            index_version: pack::index::Version::V2,
            object_hash: gix_hash::Kind::Sha1,
        };
        let odb_opts = gix_odb::store::init::Options {
            slots: gix_odb::store::init::Slots::default(),
            object_hash: gix_hash::Kind::Sha1,
            use_multi_pack_index: true,
            current_dir: Some(self.git_dir.clone()),
        };
        let thickener = Arc::new(gix_odb::Store::at_opts(
            self.git_dir.join("objects"),
            [],
            odb_opts,
        )?);
        let thickener = thickener.to_handle_arc();
        Ok(pack::Bundle::write_to_directory(
            pack,
            Some(self.git_dir.join("objects").join("pack")),
            progress,
            &self.interrupt,
            Some(Box::new(move |oid, buf| thickener.find(oid, buf).ok())),
            options,
        )?)
    }
}

pub struct Config {
    pub wants: Vec<ObjectId>,
    pub haves: Vec<ObjectId>,
}

pub struct Fetch {
    config: Config,
    pack_writer: PackWriter,
    out: FetchOut,
}

pub struct FetchOut {
    pub refs: Vec<Ref>,
    pub pack: Option<pack::bundle::write::Outcome>,
}

impl<'a> Delegate for &'a mut Fetch {
    fn receive_pack(
        &mut self,
        input: impl io::BufRead,
        progress: impl Progress,
        _refs: &[handshake::Ref],
        previous_response: &fetch::Response,
    ) -> io::Result<()> {
        self.out
            .refs
            .extend(previous_response.wanted_refs().iter().map(
                |fetch::response::WantedRef { id, path }| Ref::Direct {
                    full_ref_name: path.clone(),
                    object: *id,
                },
            ));
        let pack = self
            .pack_writer
            .write_pack(input, progress)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        self.out.pack = Some(pack);
        Ok(())
    }
}

impl<'a> DelegateBlocking for &'a mut Fetch {
    fn negotiate(
        &mut self,
        _refs: &[handshake::Ref],
        arguments: &mut fetch::Arguments,
        _previous_response: Option<&fetch::Response>,
    ) -> io::Result<fetch::Action> {
        for oid in &self.config.wants {
            arguments.want(oid);
        }

        for oid in &self.config.haves {
            arguments.have(oid);
        }

        // N.b. sends `done` packet
        Ok(fetch::Action::Cancel)
    }

    fn prepare_ls_refs(
        &mut self,
        _server: &client::Capabilities,
        _arguments: &mut Vec<BString>,
        _features: &mut Vec<(&str, Option<Cow<'_, str>>)>,
    ) -> io::Result<ls_refs::Action> {
        // We perform ls-refs elsewhere
        Ok(ls_refs::Action::Skip)
    }

    fn prepare_fetch(
        &mut self,
        _version: Protocol,
        _server: &client::Capabilities,
        _features: &mut Vec<(&str, Option<Cow<'_, str>>)>,
        _refs: &[handshake::Ref],
    ) -> io::Result<fetch::Action> {
        if self.config.wants.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "empty fetch"));
        }
        Ok(fetch::Action::Continue)
    }
}

#[allow(clippy::result_large_err)]
pub fn run<R, W>(
    git_dir: PathBuf,
    max_threads: Option<usize>,
    config: Config,
    mut conn: Connection<R, W>,
    outcome: &handshake::Outcome,
) -> Result<FetchOut, Error>
where
    R: io::Read,
    W: io::Write,
{
    log::debug!(target: "fetch", "performing fetch");
    // TODO(finto): not sure what the agent should be, possibly radicle-node + version
    let agent = "2.36.2";

    // TODO: I think this is supposed to be used in a threaded
    // environment so it might need to be passed in via the caller.
    let interrupt = Arc::new(AtomicBool::new(false));

    let mut delegate = Fetch {
        config,
        pack_writer: PackWriter {
            git_dir,
            interrupt,
            max_threads,
        },
        out: FetchOut {
            refs: Vec::new(),
            pack: None,
        },
    };

    let handshake::Outcome {
        server_protocol_version: protocol,
        refs: _refs,
        capabilities,
    } = outcome;
    let agent = agent_name(agent);
    let fetch = gix_protocol::Command::Fetch;

    let mut features = fetch.default_features(*protocol, capabilities);
    match (&mut delegate).prepare_fetch(*protocol, capabilities, &mut features, &[]) {
        Ok(fetch::Action::Continue) => {
            // FIXME: this is a private function in gitoxide
            // fetch.validate_argument_prefixes_or_panic(protocol, &capabilities, &[], &features)
        }
        // N.b. we always return Action::Continue
        Ok(fetch::Action::Cancel) => unreachable!(),
        Err(err) => {
            indicate_end_of_interaction(&mut conn)?;
            return Err(err.into());
        }
    }

    gix_protocol::fetch::Response::check_required_features(*protocol, &features)?;
    features.push(("agent", Some(Cow::Owned(agent))));
    let mut args = fetch::Arguments::new(*protocol, features);

    let mut previous_response = None::<fetch::Response>;
    'negotiation: loop {
        let action = (&mut delegate).negotiate(&[], &mut args, previous_response.as_ref())?;
        let mut reader = args.send(&mut conn, action == fetch::Action::Cancel)?;
        let response = fetch::Response::from_line_reader(*protocol, &mut reader)?;
        previous_response = if response.has_pack() {
            (&mut delegate).receive_pack(reader, progress::Discard, &[], &response)?;
            break 'negotiation;
        } else {
            match action {
                fetch::Action::Cancel => break 'negotiation,
                fetch::Action::Continue => Some(response),
            }
        }
    }
    if matches!(protocol, Protocol::V2)
        && matches!(conn.mode, FetchConnection::TerminateOnSuccessfulCompletion)
    {
        indicate_end_of_interaction(&mut conn)?;
    }

    Ok(delegate.out)
}

fn agent_name(name: impl Into<String>) -> String {
    let mut name = name.into();
    if !name.starts_with("git/") {
        name.insert_str(0, "git/");
    }
    name
}
