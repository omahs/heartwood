use std::collections::{BTreeMap, BTreeSet};

use gix_protocol::handshake;
use radicle_crypto::{PublicKey, Signer};
use thiserror::Error;

use crate::gix::refdb::{self, Updated};
use crate::gix::refdb::{Applied, SymrefTarget, Update};
use crate::sigrefs::RemoteRefs;
use crate::state::FetchState;
use crate::transport::ConnectionStream;
use crate::{identity, refs, sigrefs, stage, state, validation, Handle, Identities, Tracking};

pub const FETCH_SPECIAL_LIMIT: u64 = 1024 * 1024 * 5;
pub const FETCH_REFS_LIMIT: u64 = 1024 * 1024 * 1024 * 5;

#[derive(Clone, Copy, Debug)]
pub struct FetchLimit {
    pub special: u64,
    pub refs: u64,
}

impl Default for FetchLimit {
    fn default() -> Self {
        Self {
            special: FETCH_SPECIAL_LIMIT,
            refs: FETCH_REFS_LIMIT,
        }
    }
}

pub struct FetchResult {
    pub applied: Applied<'static>,
    pub requires_confirmation: bool,
    pub validation: Vec<validation::Validation>,
}

impl FetchResult {
    pub fn rejected(&self) -> impl Iterator<Item = &Update<'static>> {
        self.applied.rejected.iter()
    }

    pub fn updated(&self) -> impl Iterator<Item = &Updated> {
        self.applied.updated.iter()
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Update(#[from] refdb::error::Update),

    #[error(transparent)]
    Validation(#[from] validation::Error),

    #[error("failed to load all signed references")]
    RemoteRefsLoad(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("failed to load signed references for {remote}")]
    SigRefsLoad {
        remote: PublicKey,
        #[source]
        err: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    #[error("failed to update signed references")]
    SigRefsUpdate(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    State(#[from] state::Error),

    #[error("failed to get tracked peers")]
    Tracking(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

pub(crate) fn exchange<G, C, S>(
    state: &mut FetchState,
    handle: &mut Handle<G, C, S>,
    handshake: &handshake::Outcome,
    limit: FetchLimit,
    anchor: impl identity::Verified,
    remote: PublicKey,
) -> Result<FetchResult, Error>
where
    G: Signer,
    C: Tracking + Identities + sigrefs::Store,
    S: ConnectionStream,
{
    let local = *handle.local();

    let delegates = anchor
        .delegates()
        .iter()
        .filter(|id| **id != local)
        .copied()
        .collect::<BTreeSet<_>>();
    let tracked = handle
        .context
        .tracked()
        .map_err(|e| Error::Tracking(e.into()))?;

    let trusted: BTreeMap<PublicKey, bool> = tracked
        .remotes
        .iter()
        .filter_map(|id| {
            if !delegates.contains(id) {
                Some((*id, false))
            } else {
                None
            }
        })
        .chain(delegates.iter().map(|id| (*id, true)))
        .collect();

    log::info!("fetching verification refs");
    let initial = stage::Fetch {
        local,
        remote,
        delegates: delegates.clone(),
        tracked,
        limit: limit.special,
    };
    log::debug!("{initial:?}");
    state.step(handle, handshake, &initial)?;

    log::info!("loading sigrefs");
    let signed_refs = RemoteRefs::load(
        &handle.context,
        sigrefs::Select {
            must: &delegates,
            may: &trusted
                .keys()
                .filter(|id| !delegates.contains(id))
                .copied()
                .collect(),
        },
    )
    .map_err(|e| Error::RemoteRefsLoad(e.into()))?;
    log::debug!("{signed_refs:?}");

    // NOTE: we may or may not want this step
    // let requires_confirmation = {
    //     log::info!("setting up local rad/ hierarchy");
    //     let shim = state.as_shim(cx);
    //     match ids::newest(&shim, &delegates_sans_local)? {
    //         None => false,
    //         Some((their_id, theirs)) => match rad::newer(&shim, Some(anchor), theirs)? {
    //             Err(error::ConfirmationRequired) => true,
    //             Ok(newest) => {
    //                 let rad::Rad { mut track, up } = match newest {
    //                     Left(ours) => rad::setup(&shim, None, &ours)?,
    //                     Right(theirs) => rad::setup(&shim, Some(their_id), &theirs)?,
    //                 };

    //                 state.trackings_mut().append(&mut track);
    //                 state.update_all(up);

    //                 false
    //             }
    //         },
    //     }
    // };

    // Update identity tips already, we will only be looking at sigrefs from now
    // on. Can improve concurrency.
    log::info!("updating identity tips");
    let mut applied = {
        let pending = state.updates_mut();

        // `Vec::drain_filter` would help here
        let mut tips = Vec::new();
        let mut i = 0;
        while i < pending.len() {
            match &pending[i] {
                Update::Direct { name, .. } if name.ends_with(refs::REFS_RAD_ID.as_str()) => {
                    tips.push(pending.swap_remove(i));
                }
                Update::Symbolic {
                    target: SymrefTarget { name, .. },
                    ..
                } if name.ends_with(refs::REFS_RAD_ID.as_str()) => {
                    tips.push(pending.swap_remove(i));
                }
                _ => {
                    i += 1;
                }
            }
        }
        handle.refdb.update(tips)?
    };

    state.clear_rad_refs();

    let fetch_refs = stage::Refs {
        local,
        remote,
        trusted: signed_refs,
        limit: limit.refs,
    };
    log::info!("fetching data");
    log::debug!("{fetch_refs:?}");
    state.step(handle, handshake, &fetch_refs)?;

    let signed_refs = fetch_refs.trusted;

    log::info!("updating tips");
    applied.append(&mut handle.refdb.update(state.updates_mut().drain(..))?);
    for u in &applied.updated {
        log::debug!("applied {:?}", u);
    }

    log::info!("updating signed refs");
    handle
        .context
        .update(&handle.signer)
        .map_err(|e| Error::SigRefsUpdate(e.into()))?;

    let mut warnings = Vec::new();
    log::debug!("{signed_refs:?}");
    log::info!("validating signed trees");
    for (remote, refs) in &signed_refs {
        let ws = validation::validate(&handle.refdb, *remote, refs)?;
        debug_assert!(
            ws.is_empty(),
            "expected no warnings for {remote}, but got {ws:?}",
        );
        warnings.extend(ws);
    }

    log::info!("validating remote trees");
    for remote in signed_refs.keys() {
        if *remote == local {
            continue;
        }
        log::debug!("remote {}", remote);
        let refs = handle
            .context
            .load(remote)
            .map_err(|e| Error::SigRefsLoad {
                remote: *remote,
                err: e.into(),
            })?;

        match refs {
            None => warnings.push(validation::Validation::NoData(*remote)),
            Some(refs) => {
                let ws = validation::validate(&handle.refdb, *remote, &refs)?;
                debug_assert!(
                    ws.is_empty(),
                    "expected no warnings for remote {remote}, but got {ws:?}",
                );
                warnings.extend(ws);
            }
        }
    }
    Ok(FetchResult {
        applied,
        requires_confirmation: false,
        validation: warnings,
    })
}
