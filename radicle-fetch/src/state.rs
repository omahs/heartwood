use std::{collections::BTreeMap, io};

use gix_hash::ObjectId;
use gix_protocol::handshake;
use radicle_crypto::PublicKey;
use thiserror::Error;

use crate::gix::refdb::{self, Applied, Update};
use crate::identity::Identities;
use crate::stage::{error, Step};
use crate::transport::WantsHaves;
use crate::{refs, transport, Handle};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Layout(#[from] error::Layout),
    #[error(transparent)]
    Prepare(#[from] error::Prepare),
    #[error(transparent)]
    Reload(#[from] refdb::error::Reload),
    #[error(transparent)]
    WantsHaves(#[from] error::WantsHaves),
}

type IdentityTips = BTreeMap<PublicKey, ObjectId>;
type SigrefTips = BTreeMap<PublicKey, ObjectId>;

#[derive(Default)]
pub struct FetchState {
    refs: refdb::InMemory,
    ids: IdentityTips,
    sigrefs: SigrefTips,
    tips: Vec<Update<'static>>,
    max_threads: Option<usize>,
}

impl FetchState {
    pub fn id_tips(&self) -> &IdentityTips {
        &self.ids
    }

    pub fn updates_mut(&mut self) -> &mut Vec<Update<'static>> {
        &mut self.tips
    }

    pub fn clear_rad_refs(&mut self) {
        self.ids.clear();
        self.sigrefs.clear();
    }

    pub fn update_all<'a, I>(&mut self, other: I) -> Applied<'a>
    where
        I: IntoIterator<Item = Update<'a>>,
    {
        let mut ap = Applied::default();
        for up in other {
            self.tips.push(up.clone().into_owned());
            ap.append(&mut self.refs.update(Some(up)));
        }
        ap
    }
}

impl FetchState {
    pub(crate) fn step<G, C, S, F>(
        &mut self,
        handle: &mut Handle<G, C, S>,
        handshake: &handshake::Outcome,
        step: &F,
    ) -> Result<(), Error>
    where
        C: Identities,
        S: transport::ConnectionStream,
        F: Step,
    {
        handle.refdb.reload()?;
        let refs = match step.ls_refs() {
            Some(refs) => handle
                .transport
                .ls_refs(refs.into(), handshake)?
                .into_iter()
                .filter_map(|r| step.ref_filter(r))
                .collect::<Vec<_>>(),
            None => vec![],
        };
        log::debug!(target: "fetch", "received refs {:?}", refs);
        step.pre_validate(&refs)?;
        match step.wants_haves(&handle.refdb, &refs)? {
            Some(WantsHaves { wants, haves }) => {
                handle
                    .transport
                    .fetch(wants, haves, handshake, self.max_threads)?;
            }
            None => log::info!("nothing to fetch"),
        };

        for r in &refs {
            if let Some(rad) = r.name.suffix.as_ref().left() {
                match rad {
                    refs::Special::Id => {
                        self.ids.insert(*r.remote(), r.tip);
                    }

                    refs::Special::SignedRefs => {
                        self.sigrefs.insert(*r.remote(), r.tip);
                    }
                }
            }
        }

        let up = step.prepare(self, &handle.refdb, &handle.context, &refs)?;
        self.update_all(up.tips.into_iter().map(|u| u.into_owned()));

        Ok(())
    }
}
