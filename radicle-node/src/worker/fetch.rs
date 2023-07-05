mod context;
pub use context::Context;

// TODO: delete this
// mod refspecs;
// pub use refspecs::SpecialRefs;

pub mod error;

use std::collections::HashSet;

use radicle::crypto::{PublicKey, Signer};
use radicle::fetch::gix::refdb::{Updated, UserInfo};
use radicle::fetch::FetchLimit;

use radicle::prelude::{Id, NodeId};
use radicle::storage::git::Repository;

use radicle::storage::RefUpdate;
use radicle::storage::{ReadStorage, WriteStorage};
use radicle::Storage;

use crate::service;

use super::channels::Tunnel;

pub struct Handle<G> {
    inner: radicle::fetch::Handle<G, Context, Tunnel>,
    exists: bool,
}

impl<G: Signer> Handle<G> {
    pub fn new(
        rid: Id,
        signer: G,
        info: UserInfo,
        storage: &Storage,
        tracking: service::tracking::Config,
        tunnel: Tunnel,
    ) -> Result<Self, error::Handle> {
        let exists = storage.contains(&rid)?;
        let repo = if exists {
            storage.repository(rid)?
        } else {
            storage.create(rid)?
        };
        let git_dir = repo.backend.path().to_path_buf();
        let context = Context::new(tracking, repo);
        let inner = radicle::fetch::handle(
            signer,
            git_dir,
            info,
            rid.canonical().into(),
            context,
            tunnel,
        )
        .expect("TODO");

        Ok(Self { inner, exists })
    }

    pub fn fetch(&mut self, limit: FetchLimit, remote: PublicKey) -> Result<Updates, error::Fetch> {
        let result = if self.exists {
            radicle::fetch::pull(&mut self.inner, limit, remote)
        } else {
            radicle::fetch::clone(&mut self.inner, limit, remote)
        }?;

        for warn in &result.validation {
            log::warn!(target: "worker", "Validation error: {}", warn);
        }

        for rejected in result.rejected() {
            log::warn!(target: "worker", "Rejected update for {}", rejected.refname())
        }

        Ok(as_ref_updates(
            &self.inner.context().repository,
            result.applied.updated,
        )?)
    }
}

#[derive(Default)]
pub struct Updates {
    pub refs: Vec<RefUpdate>,
    pub namespaces: HashSet<NodeId>,
}

impl From<Updates> for (Vec<RefUpdate>, HashSet<NodeId>) {
    fn from(Updates { refs, namespaces }: Updates) -> Self {
        (refs, namespaces)
    }
}

fn as_ref_updates(
    repo: &Repository,
    updated: impl IntoIterator<Item = Updated>,
) -> Result<Updates, radicle::git::raw::Error> {
    use radicle::fetch::gix::oid;

    // TODO: delete this and figure out how to get the real Oids
    let zero = radicle::git::raw::Oid::zero();

    updated
        .into_iter()
        .try_fold(Updates::default(), |mut updates, update| match update {
            Updated::Direct { name, target } => {
                if let Some(ns) = name
                    .to_namespaced()
                    .and_then(|ns| ns.namespace().as_str().parse::<PublicKey>().ok())
                {
                    updates.namespaces.insert(ns);
                }
                updates.refs.push(RefUpdate::Updated {
                    name,
                    old: zero.into(),
                    new: oid::to_oid(target),
                });
                Ok(updates)
            }
            Updated::Symbolic { name, target } => {
                let new = repo.backend.refname_to_id(target.as_str())?;
                if let Some(ns) = name
                    .to_namespaced()
                    .and_then(|ns| ns.namespace().as_str().parse::<PublicKey>().ok())
                {
                    updates.namespaces.insert(ns);
                }
                updates.refs.push(RefUpdate::Updated {
                    name,
                    old: zero.into(),
                    new: new.into(),
                });

                Ok(updates)
            }
            Updated::Prune { name } => {
                if let Some(ns) = name
                    .to_namespaced()
                    .and_then(|ns| ns.namespace().as_str().parse::<PublicKey>().ok())
                {
                    updates.namespaces.insert(ns);
                }
                updates.refs.push(RefUpdate::Deleted {
                    name,
                    oid: zero.into(),
                });
                Ok(updates)
            }
        })
}
