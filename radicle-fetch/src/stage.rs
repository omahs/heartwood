use std::collections::{BTreeMap, BTreeSet, HashSet};

use bstr::BString;
use either::Either;
use git_ext::ref_format::name::component;
use git_ext::ref_format::{refname, Component, Namespaced, Qualified};
use gix_protocol::handshake::Ref;
use nonempty::NonEmpty;
use radicle_crypto::PublicKey;

use crate::gix::refdb;
use crate::gix::refdb::{Policy, Refdb, Update, Updates};
use crate::identity::{Identities, Verified as _};
use crate::refs::ReceivedRef;
use crate::sigrefs;
use crate::state::FetchState;
use crate::transport::{WantsHaves, WantsHavesBuilder};
use crate::{refs, tracking};

pub mod error {
    use radicle_crypto::PublicKey;
    use radicle_git_ext::ref_format::RefString;
    use thiserror::Error;

    use crate::gix::refdb;

    #[derive(Debug, Error)]
    pub enum Layout {
        #[error("missing required refs: {0:?}")]
        MissingRequiredRefs(Vec<String>),
    }

    #[derive(Debug, Error)]
    pub enum Prepare {
        #[error("refdb scan error")]
        Scan {
            #[source]
            err: Box<dyn std::error::Error + Send + Sync + 'static>,
        },
        #[error("verification of rad/id for {remote} failed")]
        Verification {
            remote: PublicKey,
            #[source]
            err: Box<dyn std::error::Error + Send + Sync + 'static>,
        },
    }

    #[derive(Debug, Error)]
    pub enum WantsHaves {
        #[error(transparent)]
        Find(#[from] refdb::error::Find),
        #[error("expected namespaced ref {0}")]
        NotNamespaced(RefString),
    }
}

pub(crate) trait Step {
    /// Validate that all advertised refs conform to an expected layout.
    ///
    /// The supplied `refs` are `ls-ref`-advertised refs filtered through
    /// [`crate::Negotiation::ref_filter`].
    fn pre_validate(&self, refs: &[ReceivedRef]) -> Result<(), error::Layout>;

    /// If and how to perform `ls-refs`.
    fn ls_refs(&self) -> Option<NonEmpty<BString>>;

    /// Filter a remote-advertised [`Ref`].
    ///
    /// Return `Some` if the ref should be considered, `None` otherwise. This
    /// method may be called with the response of `ls-refs`, the `wanted-refs`
    /// of a `fetch` response, or both.
    fn ref_filter(&self, r: Ref) -> Option<ReceivedRef>;

    /// Assemble the `want`s and `have`s for a `fetch`, retaining the refs which
    /// would need updating after the `fetch` succeeds.
    ///
    /// The `refs` are the advertised refs from executing `ls-refs`, filtered
    /// through [`Negotiation::ref_filter`].
    fn wants_haves(
        &self,
        refdb: &Refdb,
        refs: &[ReceivedRef],
    ) -> Result<Option<WantsHaves>, error::WantsHaves> {
        let mut builder = WantsHavesBuilder::default();
        builder.add(refdb, refs)?;
        Ok(builder.build())
    }

    fn prepare<'a, I>(
        &self,
        s: &FetchState,
        refdb: &Refdb,
        ids: &I,
        refs: &'a [ReceivedRef],
    ) -> Result<Updates<'a>, error::Prepare>
    where
        I: Identities;
}

#[derive(Debug)]
pub struct Clone {
    pub remote: PublicKey,
    pub limit: u64,
}

impl Step for Clone {
    fn pre_validate(&self, refs: &[ReceivedRef]) -> Result<(), error::Layout> {
        ensure_refs(
            special_refs(self.remote).collect(),
            refs.iter()
                .map(|r| r.name.namespaced().to_string().into())
                .collect(),
        )
    }

    fn ls_refs(&self) -> Option<NonEmpty<BString>> {
        NonEmpty::collect(special_refs(self.remote))
    }

    fn ref_filter(&self, r: Ref) -> Option<ReceivedRef> {
        let (name, tip) = refdb::unpack_ref(r);
        match refs::Refname::try_from(name).ok()? {
            refname @ refs::Refname {
                suffix: Either::Left(_),
                ..
            } => Some(ReceivedRef::new(tip, refname)),
            _ => None,
        }
    }

    fn prepare<'a, I>(
        &self,
        s: &FetchState,
        _refdb: &Refdb,
        ids: &I,
        refs: &'a [ReceivedRef],
    ) -> Result<Updates<'a>, error::Prepare>
    where
        I: Identities,
    {
        let verified = ids
            .verified(
                *s.id_tips()
                    .get(&self.remote)
                    .expect("ensured we got rad/id ref"),
            )
            .map_err(|err| error::Prepare::Verification {
                remote: self.remote,
                err: Box::new(err),
            })?;
        let tips = if verified.delegates().contains(&self.remote) {
            refs.iter()
                .filter_map(ReceivedRef::as_verification_ref_update)
                .collect()
        } else {
            vec![]
        };

        Ok(Updates { tips })
    }
}

#[derive(Debug)]
pub struct Fetch {
    pub local: PublicKey,
    pub remote: PublicKey,
    pub tracked: tracking::Tracked,
    pub delegates: BTreeSet<PublicKey>,
    pub limit: u64,
}

impl Step for Fetch {
    fn pre_validate(&self, refs: &[ReceivedRef]) -> Result<(), error::Layout> {
        ensure_refs(
            self.delegates
                .iter()
                .filter(|id| **id != self.local)
                .flat_map(|id| special_refs(*id))
                .collect(),
            refs.iter()
                .map(|r| r.name.namespaced().to_string().into())
                .collect(),
        )
    }

    fn ls_refs(&self) -> Option<NonEmpty<BString>> {
        match &self.tracked.scope {
            tracking::Scope::All => Some(NonEmpty::new("refs/namespaces/*".into())),
            tracking::Scope::Trusted => NonEmpty::collect(
                self.tracked
                    .remotes
                    .iter()
                    .flat_map(|remote| special_refs(*remote)),
            ),
        }
    }

    fn ref_filter(&self, r: Ref) -> Option<ReceivedRef> {
        let (refname, tip) = refdb::unpack_ref(r);
        let refname = refs::Refname::try_from(refname).ok()?;
        Some(ReceivedRef::new(tip, refname))
    }

    fn prepare<'a, I>(
        &self,
        _s: &FetchState,
        _refdb: &Refdb,
        ids: &I,
        refs: &'a [ReceivedRef],
    ) -> Result<Updates<'a>, error::Prepare>
    where
        I: Identities,
    {
        verification_refs(&self.local, ids, refs, |remote_id| {
            self.delegates.contains(remote_id)
        })
    }
}

#[derive(Debug)]
pub struct Refs {
    pub local: PublicKey,
    pub remote: PublicKey,
    pub trusted: sigrefs::RemoteRefs,
    pub limit: u64,
}

impl Step for Refs {
    fn pre_validate(&self, _refs: &[ReceivedRef]) -> Result<(), error::Layout> {
        Ok(())
    }

    fn ls_refs(&self) -> Option<NonEmpty<BString>> {
        NonEmpty::collect(self.trusted.keys().flat_map(|remote| special_refs(*remote)))
    }

    fn ref_filter(&self, r: Ref) -> Option<ReceivedRef> {
        let (refname, tip) = refdb::unpack_ref(r);
        match refs::Refname::try_from(refname).ok()? {
            refname @ refs::Refname {
                remote,
                suffix: Either::Left(_),
            } if self.trusted.contains_key(&remote) => Some(ReceivedRef::new(tip, refname)),
            _ => None,
        }
    }

    fn wants_haves(
        &self,
        refdb: &Refdb,
        refs: &[ReceivedRef],
    ) -> Result<Option<WantsHaves>, error::WantsHaves> {
        let mut builder = WantsHavesBuilder::default();

        for (remote, refs) in &self.trusted {
            for (name, tip) in refs {
                let refname = Qualified::from_refstr(name)
                    .map(|suffix| refs::Refname::remote(*remote, suffix).namespaced())
                    .ok_or_else(|| error::WantsHaves::NotNamespaced(name.to_owned()))?;
                let want = match refdb.refname_to_id(refname)? {
                    Some(oid) => {
                        let want = *tip != oid && !refdb.contains(tip);
                        builder.have(oid);
                        want
                    }
                    None => !refdb.contains(tip),
                };
                if want {
                    builder.want(*tip);
                }
            }
        }

        builder.add(refdb, refs)?;
        Ok(builder.build())
    }

    fn prepare<'a, I>(
        &self,
        _s: &FetchState,
        refdb: &Refdb,
        _ids: &I,
        _refs: &'a [ReceivedRef],
    ) -> Result<Updates<'a>, error::Prepare>
    where
        I: Identities,
    {
        let mut tips = {
            let sz = self.trusted.values().map(|rs| rs.refs.len()).sum();
            Vec::with_capacity(sz)
        };

        for (remote, refs) in &self.trusted {
            let mut signed = HashSet::with_capacity(refs.refs.len());
            for (name, tip) in refs {
                let tracking: Namespaced<'_> = Qualified::from_refstr(name)
                    .map(|q| refs::Refname::remote(*remote, q).namespaced())
                    .expect("we checked sigrefs well-formedness in wants_refs already");
                signed.insert(tracking.clone());
                tips.push(Update::Direct {
                    name: tracking,
                    target: tip.as_ref().to_owned(),
                    no_ff: Policy::Allow,
                });
            }

            // Prune refs not in signed
            let prefix = refname!("refs/namespaces").join(Component::from(remote));
            let prefix_rad = prefix.join(component!("rad"));
            let scan_err = |e: refdb::error::Scan| error::Prepare::Scan { err: e.into() };
            for known in refdb.scan(Some(prefix.as_str())).map_err(scan_err)? {
                let refdb::Ref { name, target, .. } = known.map_err(scan_err)?;
                let ns = name.to_namespaced();
                // should only be pruning namespaced refs
                let ns = match ns {
                    Some(name) => name.to_owned(),
                    None => continue,
                };

                // 'rad/' refs are never subject to pruning
                if ns.starts_with(prefix_rad.as_str()) {
                    continue;
                }

                if !signed.contains(&ns) {
                    tips.push(Update::Prune {
                        name: ns,
                        prev: target,
                    });
                }
            }
        }

        Ok(Updates { tips })
    }
}

fn verification_refs<'a, F>(
    local_id: &PublicKey,
    ids: &impl Identities,
    refs: &'a [ReceivedRef],
    is_delegate: F,
) -> Result<Updates<'a>, error::Prepare>
where
    F: Fn(&PublicKey) -> bool,
{
    use either::Either::*;

    let grouped: BTreeMap<&PublicKey, Vec<&ReceivedRef>> = refs
        .iter()
        .filter_map(|r| {
            let remote_id = &r.name.remote;
            (remote_id != local_id).then_some((remote_id, r))
        })
        .fold(BTreeMap::new(), |mut acc, (remote_id, r)| {
            acc.entry(remote_id).or_insert_with(Vec::new).push(r);
            acc
        });

    let mut updates = Updates {
        tips: Vec::with_capacity(refs.len()),
    };

    for (remote_id, refs) in grouped {
        let is_delegate = is_delegate(remote_id);

        let mut tips_inner = Vec::with_capacity(refs.len());
        for r in refs {
            match &r.name.suffix {
                Left(refs::Special::Id) => {
                    match ids.verified(r.tip) {
                        Err(e) if is_delegate => {
                            return Err(error::Prepare::Verification {
                                remote: *remote_id,
                                err: e.into(),
                            })
                        }
                        Err(e) => {
                            log::warn!("error verifying non-delegate id {remote_id}: {e}");
                            // Verification error for a non-delegate taints
                            // all refs for this remote_id
                            tips_inner.clear();
                            break;
                        }

                        Ok(_) => {
                            if let Some(u) = r.as_verification_ref_update() {
                                tips_inner.push(u)
                            }
                        }
                    }
                }

                Left(_) => {
                    if let Some(u) = r.as_verification_ref_update() {
                        tips_inner.push(u)
                    }
                }

                Right(_) => continue,
            }
        }

        updates.tips.append(&mut tips_inner);
    }

    Ok(updates)
}

fn special_refs(remote: PublicKey) -> impl Iterator<Item = BString> {
    [
        refs::Refname::rad_id(remote).to_string().into(),
        refs::Refname::rad_sigrefs(remote).to_string().into(),
    ]
    .into_iter()
}

fn ensure_refs<T>(required: BTreeSet<T>, wants: BTreeSet<T>) -> Result<(), error::Layout>
where
    T: Ord + ToString,
{
    if wants.is_empty() {
        return Ok(());
    }

    let diff = required.difference(&wants).collect::<Vec<_>>();

    if diff.is_empty() {
        Ok(())
    } else {
        Err(error::Layout::MissingRequiredRefs(
            diff.into_iter().map(|ns| ns.to_string()).collect(),
        ))
    }
}
