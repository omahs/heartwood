use std::borrow::Cow;
use std::io::{self, BufRead};

use bstr::ByteSlice;
use gix_features::progress::Progress;
use gix_protocol::fetch::{self, Delegate, DelegateBlocking};
use gix_protocol::handshake::{self, Ref};
use gix_protocol::transport::Protocol;
use gix_protocol::{ls_refs, Command};
use gix_transport::bstr::{BString, ByteVec};
use gix_transport::client::{self, TransportV2Ext};

use super::{indicate_end_of_interaction, Connection};

pub struct Config {
    pub repo: BString,
    pub extra_params: Vec<(String, Option<String>)>,
    pub prefixes: Vec<BString>,
}

pub struct LsRefs {
    config: Config,
    refs: Vec<Ref>,
}

impl LsRefs {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            refs: Vec::new(),
        }
    }
}

impl DelegateBlocking for LsRefs {
    fn handshake_extra_parameters(&self) -> Vec<(String, Option<String>)> {
        self.config.extra_params.clone()
    }

    fn prepare_ls_refs(
        &mut self,
        _caps: &client::Capabilities,
        args: &mut Vec<BString>,
        _: &mut Vec<(&str, Option<Cow<'_, str>>)>,
    ) -> io::Result<ls_refs::Action> {
        for prefix in &self.config.prefixes {
            let mut arg = BString::from("ref-prefix ");
            arg.push_str(prefix);
            args.push(arg)
        }
        Ok(ls_refs::Action::Continue)
    }

    fn prepare_fetch(
        &mut self,
        _: Protocol,
        _: &client::Capabilities,
        _: &mut Vec<(&str, Option<Cow<'_, str>>)>,
        refs: &[Ref],
    ) -> io::Result<fetch::Action> {
        self.refs.extend_from_slice(refs);
        Ok(fetch::Action::Cancel)
    }

    fn negotiate(
        &mut self,
        _: &[Ref],
        _: &mut fetch::Arguments,
        _: Option<&fetch::Response>,
    ) -> io::Result<fetch::Action> {
        unreachable!("`negotiate` called even though no `fetch` command was sent")
    }
}

impl Delegate for LsRefs {
    fn receive_pack(
        &mut self,
        _: impl BufRead,
        _: impl Progress,
        _: &[Ref],
        _: &fetch::Response,
    ) -> io::Result<()> {
        unreachable!("`receive_pack` called even though no `fetch` command was sent")
    }
}

pub fn run<R, W>(
    config: Config,
    handshake: &handshake::Outcome,
    mut conn: Connection<R, W>,
) -> Result<Vec<Ref>, ls_refs::Error>
where
    R: io::Read,
    W: io::Write,
{
    log::debug!(target: "fetch", "performing ls-refs: {:?}", config.prefixes);
    let mut delegate = LsRefs::new(config);
    let handshake::Outcome {
        server_protocol_version: protocol,
        capabilities,
        ..
    } = handshake;

    if protocol != &Protocol::V2 {
        return Err(ls_refs::Error::Io(io::Error::new(
            io::ErrorKind::Other,
            "expected protocol version 2",
        )));
    }

    let ls = Command::LsRefs;
    let mut features = ls.default_features(Protocol::V2, capabilities);
    // N.b. copied from gitoxide
    let mut args = vec![
        b"symrefs".as_bstr().to_owned(),
        b"peel".as_bstr().to_owned(),
    ];
    if capabilities
        .capability("ls-refs")
        .and_then(|cap| cap.supports("unborn"))
        .unwrap_or_default()
    {
        args.push("unborn".into());
    }
    let refs = match delegate.prepare_ls_refs(capabilities, &mut args, &mut features) {
        Ok(ls_refs::Action::Skip) => Vec::new(),
        Ok(ls_refs::Action::Continue) => {
            // FIXME: this is a private function
            // ls.validate_argument_prefixes_or_panic(Protocol::V2, capabilities, &args, &features);
            features.push(("agent", Some(Cow::Owned("git/2.36.2".to_string()))));
            let mut remote_refs = conn.invoke(
                ls.as_str(),
                features.clone().into_iter(),
                args.is_empty().then_some(args.into_iter()),
            )?;
            handshake::refs::from_v2_refs(&mut remote_refs)?
        }
        Err(err) => {
            indicate_end_of_interaction(&mut conn)?;
            return Err(err.into());
        }
    };
    indicate_end_of_interaction(&mut conn)?;

    Ok(refs)
}
