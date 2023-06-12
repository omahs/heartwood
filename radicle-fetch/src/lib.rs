pub mod gix;

use std::{io, path::PathBuf};

use bstr::BString;
use gix::refdb::UserInfo;
pub use gix::{Odb, Refdb};

pub mod identity;
pub use identity::{Identities, Verified};

mod protocol;
pub use protocol::{FetchLimit, FetchResult};

pub mod refs;
pub mod sigrefs;
pub mod stage;

pub mod state;
pub use state::FetchState;

pub mod tracking;
pub use tracking::{Scope, Tracked, Tracking};

pub mod transport;
use transport::ConnectionStream;
pub use transport::Transport;

pub mod validation;

use radicle_crypto::{PublicKey, Signer};
use thiserror::Error;

extern crate radicle_git_ext as git_ext;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to perform fetch handshake")]
    Handshake {
        #[source]
        err: io::Error,
    },
    #[error("failed to load `rad/id`")]
    Identity {
        #[source]
        err: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error(transparent)]
    Protocol(#[from] protocol::Error),
    #[error("missing `rad/id`")]
    MissingRadId,
    #[error("attempted to replicate from self")]
    ReplicateSelf,
}

pub struct Handle<G, C, S> {
    signer: G,
    refdb: Refdb,
    context: C,
    transport: Transport<S>,
}

pub fn handle<G, C, S>(
    signer: G,
    git_dir: PathBuf,
    info: UserInfo,
    repo: BString,
    context: C,
    connection: S,
) -> Result<Handle<G, C, S>, gix::refdb::error::Init>
where
    S: ConnectionStream,
    C: Tracking + Identities + sigrefs::Store,
{
    let odb = Odb::new(&git_dir);
    let refdb = Refdb::new(info, odb, git_dir.clone())?;
    let transport = Transport::new(git_dir, repo, connection);
    Ok(Handle {
        signer,
        refdb,
        context,
        transport,
    })
}

impl<G, C, S> Handle<G, C, S> {
    pub fn context(&self) -> &C {
        &self.context
    }

    pub fn local(&self) -> &PublicKey
    where
        G: Signer,
    {
        self.signer.public_key()
    }
}

pub fn pull<G, C, S>(
    handle: &mut Handle<G, C, S>,
    limit: FetchLimit,
    remote: PublicKey,
) -> Result<FetchResult, Error>
where
    G: Signer,
    C: Tracking + Identities + sigrefs::Store,
    S: transport::ConnectionStream,
{
    if *handle.local() == remote {
        return Err(Error::ReplicateSelf);
    }
    let anchor = identity::current(handle.local(), &handle.context, &handle.refdb)
        .map_err(|e| Error::Identity { err: e.into() })?
        .ok_or(Error::MissingRadId)?;
    let handshake = handle
        .transport
        .handshake()
        .map_err(|err| Error::Handshake { err })?;
    Ok(protocol::exchange(
        &mut FetchState::default(),
        handle,
        &handshake,
        limit,
        anchor,
        remote,
    )?)
}

pub fn clone<G, C, S>(
    handle: &mut Handle<G, C, S>,
    limit: FetchLimit,
    remote: PublicKey,
) -> Result<FetchResult, Error>
where
    G: Signer,
    C: Tracking + Identities + sigrefs::Store,
    S: transport::ConnectionStream,
{
    log::info!("fetching initial special refs");
    if *handle.local() == remote {
        return Err(Error::ReplicateSelf);
    }
    let handshake = handle
        .transport
        .handshake()
        .map_err(|err| Error::Handshake { err })?;
    let mut state = FetchState::default();
    state
        .step(
            handle,
            &handshake,
            &stage::Clone {
                remote,
                limit: limit.special,
            },
        )
        .map_err(protocol::Error::from)?;

    let anchor = handle
        .context
        .verified(
            *state
                .id_tips()
                .get(&remote)
                .expect("missing `rad/id` after initial clone step"),
        )
        .map_err(|e| Error::Identity { err: e.into() })?;
    Ok(protocol::exchange(
        &mut state, handle, &handshake, limit, anchor, remote,
    )?)
}
