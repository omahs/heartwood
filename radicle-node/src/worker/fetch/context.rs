use std::collections::HashSet;

use radicle::fetch::gix;
use radicle::storage::git::Repository;
use thiserror::Error;

use radicle::crypto::{PublicKey, Signer};
use radicle::fetch;
use radicle::fetch::{sigrefs::Store, Identities, Tracked, Tracking};
use radicle::node::tracking;
use radicle::prelude::Id;

use crate::service;

pub struct Context {
    pub(super) config: service::tracking::Config,
    pub(super) repository: Repository,
}

impl Context {
    pub fn new(config: service::tracking::Config, repository: Repository) -> Self {
        Self { config, repository }
    }
}

#[derive(Debug, Error)]
pub enum TrackingError {
    #[error("Failed to find tracking policy for {rid}")]
    FailedPolicy {
        rid: Id,
        #[source]
        err: tracking::store::Error,
    },
    #[error("Cannot fetch {rid} as it is not tracked")]
    BlockedPolicy { rid: Id },
    #[error("Failed to get tracking nodes for {rid}")]
    FailedNodes {
        rid: Id,
        #[source]
        err: tracking::store::Error,
    },

    #[error(transparent)]
    Storage(#[from] radicle::storage::Error),

    #[error(transparent)]
    Git(#[from] radicle::git::raw::Error),

    #[error(transparent)]
    Refs(#[from] radicle::storage::refs::Error),
}

impl Store for Context {
    type LoadError = <Repository as Store>::LoadError;
    type UpdateError = <Repository as Store>::UpdateError;

    fn load(
        &self,
        remote: &PublicKey,
    ) -> Result<Option<radicle::fetch::sigrefs::Sigrefs>, Self::LoadError> {
        self.repository.load(remote)
    }

    fn update<G>(&self, signer: &G) -> Result<(), Self::UpdateError>
    where
        G: Signer,
    {
        self.repository.update(signer)
    }
}

impl Identities for Context {
    type VerifiedIdentity = <Repository as Identities>::VerifiedIdentity;
    type VerifiedError = <Repository as Identities>::VerifiedError;

    fn verified(&self, head: gix::ObjectId) -> Result<Self::VerifiedIdentity, Self::VerifiedError> {
        self.repository.verified(head)
    }
}

impl Tracking for Context {
    type Error = TrackingError;

    fn tracked(&self) -> Result<Tracked, Self::Error> {
        use TrackingError::*;

        let rid = self.repository.id;
        let entry = self
            .config
            .repo_policy(&rid)
            .map_err(|err| FailedPolicy { rid, err })?;
        match entry.policy {
            tracking::Policy::Block => {
                log::error!(target: "service", "Attempted to fetch untracked repo {rid}");
                Err(BlockedPolicy { rid })
            }
            tracking::Policy::Track => match entry.scope {
                tracking::Scope::All => Ok(Tracked {
                    scope: fetch::Scope::All,
                    // TODO: perhaps we should still only return tracked nodes
                    remotes: self
                        .repository
                        .remote_ids()?
                        .collect::<Result<HashSet<_>, _>>()?,
                }),
                tracking::Scope::Trusted => {
                    let nodes = self
                        .config
                        .node_policies()
                        .map_err(|err| FailedNodes { rid, err })?;
                    let trusted: HashSet<_> = nodes
                        .filter_map(|node| {
                            (node.policy == tracking::Policy::Track).then_some(node.id)
                        })
                        .collect();

                    Ok(Tracked {
                        scope: fetch::Scope::Trusted,
                        remotes: trusted,
                    })
                }
            },
        }
    }
}
