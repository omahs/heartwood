mod internal;

mod mem;
pub use mem::InMemory;

pub mod error;

mod update;
pub use update::{Applied, Policy, SymrefTarget, Update, Updated, Updates};

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use bstr::{BString, ByteVec};
use either::{Either, Either::*};
use gix_actor::{Signature, Time};
use gix_hash::ObjectId;
use gix_protocol::handshake;
use gix_ref::{
    file::iter::LooseThenPacked,
    transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog},
    FullName, Reference, Target,
};
use radicle_crypto::PublicKey;
use radicle_git_ext::ref_format::{Namespaced, Qualified, RefString};

use super::Odb;

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub alias: String,
    pub pk: PublicKey,
}

impl UserInfo {
    pub fn signature(&self) -> Signature {
        Signature {
            name: BString::from(self.alias.as_str()),
            email: format!("{}@{}", self.alias, self.pk).into(),
            time: Time::now_local_or_utc(),
        }
    }
}

pub struct Ref {
    pub name: Qualified<'static>,
    pub target: Either<ObjectId, Qualified<'static>>,
    pub peeled: ObjectId,
}

impl TryFrom<handshake::Ref> for Ref {
    type Error = error::RefConversion;

    fn try_from(r: handshake::Ref) -> Result<Self, Self::Error> {
        match r {
            handshake::Ref::Peeled {
                full_ref_name,
                tag,
                object,
            } => Ok(Ref {
                name: fullname_to_qualified(FullName::try_from(full_ref_name)?)?,
                target: Either::Left(tag),
                peeled: object,
            }),
            handshake::Ref::Direct {
                full_ref_name,
                object,
            } => Ok(Ref {
                name: fullname_to_qualified(FullName::try_from(full_ref_name)?)?,
                target: Either::Left(object),
                peeled: object,
            }),
            handshake::Ref::Symbolic {
                full_ref_name,
                target,
                object,
            } => Ok(Ref {
                name: fullname_to_qualified(FullName::try_from(full_ref_name)?)?,
                target: Either::Right(fullname_to_qualified(FullName::try_from(target)?)?),
                peeled: object,
            }),
            handshake::Ref::Unborn { full_ref_name, .. } => {
                Err(error::RefConversion::Unborn(full_ref_name))
            }
        }
    }
}

pub fn unpack_ref(r: handshake::Ref) -> (BString, ObjectId) {
    match r {
        handshake::Ref::Peeled {
            full_ref_name,
            object,
            ..
        }
        | handshake::Ref::Direct {
            full_ref_name,
            object,
        }
        | handshake::Ref::Symbolic {
            full_ref_name,
            object,
            ..
        } => (full_ref_name, object),
        handshake::Ref::Unborn { full_ref_name, .. } => {
            unreachable!("BUG: unborn ref {}", full_ref_name)
        }
    }
}

pub struct Scan<'a> {
    snapshot: &'a internal::Snapshot,
    odb: &'a Odb,
    inner: LooseThenPacked<'a, 'a>,
}

impl<'a> Scan<'a> {
    fn next_ref(&self, mut r: Reference) -> Result<Ref, error::Scan> {
        let peeled = self.snapshot.peel(self.odb, &mut r)?;
        let name = fullname_to_qualified(r.name)?;
        let target = match r.target {
            Target::Peeled(oid) => Either::Left(oid),
            Target::Symbolic(symref) => Either::Right(fullname_to_qualified(symref)?),
        };
        Ok(Ref {
            name,
            target,
            peeled,
        })
    }
}

impl<'a> Iterator for Scan<'a> {
    type Item = Result<Ref, error::Scan>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()?.map_err(error::Scan::from) {
            Ok(r) => Some(self.next_ref(r)),
            Err(e) => Some(Err(e)),
        }
    }
}

pub struct Refdb {
    info: UserInfo,
    odb: Odb,
    refdb: internal::Refdb,
    snapshot: internal::Snapshot,
}

impl Refdb {
    pub fn new(info: UserInfo, odb: Odb, git_dir: impl Into<PathBuf>) -> Result<Self, error::Init> {
        let refdb = internal::Refdb::open(git_dir)?;
        let snapshot = refdb.snapshot()?;

        Ok(Self {
            info,
            odb,
            refdb,
            snapshot,
        })
    }

    pub fn contains(&self, oid: impl AsRef<gix_hash::oid>) -> bool {
        self.odb.contains(oid)
    }

    pub fn scan(&self, prefix: Option<impl AsRef<Path>>) -> Result<Scan<'_>, error::Scan> {
        let inner = self.snapshot.iter(prefix)?;
        Ok(Scan {
            snapshot: &self.snapshot,
            odb: &self.odb,
            inner,
        })
    }

    pub fn refname_to_id<'a, N>(&self, refname: N) -> Result<Option<ObjectId>, error::Find>
    where
        N: Into<Namespaced<'a>>,
    {
        let name = namespaced_to_fullname(refname.into());
        match self.snapshot.find(name.as_ref().as_partial_name())? {
            None => Ok(None),
            Some(mut tip) => Ok(Some(self.snapshot.peel(&self.odb, &mut tip)?)),
        }
    }

    pub fn reload(&mut self) -> Result<(), error::Reload> {
        self.snapshot = self.refdb.snapshot()?;
        Ok(())
    }

    pub fn update<'a, I>(&mut self, updates: I) -> Result<Applied<'a>, error::Update>
    where
        I: IntoIterator<Item = Update<'a>>,
    {
        let (rejected, edits) = updates
            .into_iter()
            .map(|update| self.to_edits(update))
            .filter_map(|r| r.ok())
            .fold(
                (
                    Vec::<Update<'a>>::default(),
                    HashMap::<FullName, RefEdit>::default(),
                ),
                |(mut rejected, mut edits), e| {
                    match e {
                        Left(r) => rejected.push(r),
                        Right(e) => edits.extend(e.into_iter().map(|e| (e.name.clone(), e))),
                    }
                    (rejected, edits)
                },
            );

        // TODO(finto): Use the edits as a way of figuring out the previous values of the refs
        let txn = self.snapshot.transaction().prepare(
            edits.into_values(),
            gix_lock::acquire::Fail::Immediately,
            gix_lock::acquire::Fail::Immediately,
        )?;

        let signature = self.info.signature();
        let applied = txn
            .commit(Some(signature.to_ref()))?
            .into_iter()
            .map(|RefEdit { change, name, .. }| {
                let name = fullname_to_refstring(name)?;
                Ok(match change {
                    Change::Update { new, .. } => match new {
                        Target::Peeled(target) => Updated::Direct { name, target },
                        Target::Symbolic(target) => Updated::Symbolic {
                            name,
                            target: fullname_to_refstring(target)?,
                        },
                    },
                    Change::Delete { .. } => Updated::Prune { name },
                })
            })
            .collect::<Result<Vec<_>, error::Update>>()?;

        if !applied.is_empty() {
            self.reload()?;
        }

        Ok(Applied {
            rejected,
            updated: applied,
        })
    }

    fn to_edits<'a>(
        &self,
        update: Update<'a>,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Update> {
        match update {
            Update::Direct {
                name,
                target,
                no_ff,
            } => self.direct_edit(name, target, no_ff),
            Update::Symbolic {
                name,
                target,
                type_change,
            } => self.symbolic_edit(name, target, type_change),
            Update::Prune { name, prev } => Ok(Either::Right(vec![RefEdit {
                change: Change::Delete {
                    expected: PreviousValue::MustExistAndMatch(
                        prev.map_right(qualified_to_fullname)
                            .either(Target::Peeled, Target::Symbolic),
                    ),
                    log: RefLog::AndReference,
                },
                name: namespaced_to_fullname(name),
                deref: false,
            }])),
        }
    }

    fn direct_edit<'a>(
        &self,
        name: Namespaced<'a>,
        target: ObjectId,
        no_ff: Policy,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Update> {
        use Either::*;

        let force_create_reflog = force_reflog(&name);
        let name_ns = namespaced_to_fullname(name.clone());
        let tip = self.find_snapshot(&name_ns)?;
        match tip {
            None => Ok(Right(vec![RefEdit {
                change: Change::Update {
                    log: LogChange {
                        mode: RefLog::AndReference,
                        force_create_reflog,
                        message: "radicle: create".into(),
                    },
                    expected: PreviousValue::MustNotExist,
                    new: Target::Peeled(target),
                },
                name: name_ns,
                deref: false,
            }])),
            Some(prev) => {
                let is_ff = self.odb.is_in_ancestry_path(target, prev)?;

                if !is_ff {
                    match no_ff {
                        Policy::Abort => Err(error::Update::NonFF {
                            name: name_ns.into_inner(),
                            new: target,
                            cur: prev,
                        }),
                        Policy::Reject => Ok(Left(Update::Direct {
                            name,
                            target,
                            no_ff,
                        })),
                        Policy::Allow => Ok(Right(vec![RefEdit {
                            change: Change::Update {
                                log: LogChange {
                                    mode: RefLog::AndReference,
                                    force_create_reflog,
                                    message: "radicle: forced update".into(),
                                },
                                expected: PreviousValue::MustExistAndMatch(Target::Peeled(prev)),
                                new: Target::Peeled(target),
                            },
                            name: name_ns,
                            deref: false,
                        }])),
                    }
                } else {
                    Ok(Right(vec![RefEdit {
                        change: Change::Update {
                            log: LogChange {
                                mode: RefLog::AndReference,
                                force_create_reflog,
                                message: "radicle: fast-forward".into(),
                            },
                            expected: PreviousValue::MustExistAndMatch(Target::Peeled(prev)),
                            new: Target::Peeled(target),
                        },
                        name: name_ns,
                        deref: false,
                    }]))
                }
            }
        }
    }

    fn symbolic_edit<'a>(
        &self,
        name: Namespaced<'a>,
        target: SymrefTarget<'a>,
        type_change: Policy,
    ) -> Result<Either<Update<'a>, Vec<RefEdit>>, error::Update> {
        let name_ns = namespaced_to_fullname(name.clone());
        let src = self
            .snapshot
            .find(name_ns.as_bstr())
            .map_err(error::Find::from)?
            .map(|r| r.target);

        match src {
            Some(Target::Peeled(_)) if matches!(type_change, Policy::Abort) => {
                Err(error::Update::TypeChange(name_ns.into_inner()))
            }
            Some(Target::Peeled(_)) if matches!(type_change, Policy::Reject) => {
                Ok(Left(Update::Symbolic {
                    name,
                    target,
                    type_change,
                }))
            }

            _ => {
                let src_name = name_ns;
                let dst = self
                    .snapshot
                    .find(target.name().as_bstr())
                    .map_err(error::Find::from)?
                    .map(|r| r.target);

                let SymrefTarget {
                    name: dst_name,
                    target,
                } = target;
                let edits = match dst {
                    Some(Target::Symbolic(dst)) => {
                        return Err(error::Update::TargetSymbolic(dst.into_inner()))
                    }

                    None => {
                        let force_create_reflog = force_reflog(&dst_name);
                        let dst_name = namespaced_to_fullname(dst_name);
                        vec![
                            // Create target
                            RefEdit {
                                change: Change::Update {
                                    log: LogChange {
                                        mode: RefLog::AndReference,
                                        force_create_reflog,
                                        message: "radicle: create symref target".into(),
                                    },
                                    expected: PreviousValue::MustNotExist,
                                    new: Target::Peeled(target),
                                },
                                name: dst_name.clone(),
                                deref: false,
                            },
                            // Create source
                            RefEdit {
                                change: Change::Update {
                                    log: LogChange {
                                        mode: RefLog::AndReference,
                                        force_create_reflog,
                                        message: "radicle: create symbolic ref".into(),
                                    },
                                    expected: PreviousValue::MustNotExist,
                                    new: Target::Symbolic(dst_name),
                                },
                                name: src_name,
                                deref: false,
                            },
                        ]
                    }

                    Some(Target::Peeled(dst)) => {
                        let mut edits = Vec::with_capacity(2);

                        let is_ff = target != dst && self.odb.is_in_ancestry_path(target, dst)?;
                        let force_create_reflog = force_reflog(&dst_name);
                        let dst_name = namespaced_to_fullname(dst_name);

                        if is_ff {
                            edits.push(RefEdit {
                                change: Change::Update {
                                    log: LogChange {
                                        mode: RefLog::AndReference,
                                        force_create_reflog,
                                        message: "radicle: fast-forward symref target".into(),
                                    },
                                    expected: PreviousValue::MustExistAndMatch(Target::Peeled(dst)),
                                    new: Target::Peeled(target),
                                },
                                name: dst_name.clone(),
                                deref: false,
                            })
                        }

                        edits.push(RefEdit {
                            change: Change::Update {
                                log: LogChange {
                                    mode: RefLog::AndReference,
                                    force_create_reflog,
                                    message: "radicle: symbolic ref".into(),
                                },
                                expected: src
                                    .map(PreviousValue::MustExistAndMatch)
                                    .unwrap_or(PreviousValue::MustNotExist),
                                new: Target::Symbolic(dst_name),
                            },
                            name: src_name,
                            deref: false,
                        });
                        edits
                    }
                };

                Ok(Right(edits))
            }
        }
    }

    fn find_snapshot(&self, name: &FullName) -> Result<Option<ObjectId>, error::Find> {
        match self.snapshot.find(name.as_ref().as_partial_name())? {
            None => Ok(None),
            Some(mut tip) => Ok(Some(self.snapshot.peel(&self.odb, &mut tip)?)),
        }
    }
}

fn fullname_to_qualified(name: FullName) -> Result<Qualified<'static>, git_ext::ref_format::Error> {
    fullname_to_refstring(name).map(|name| {
        name.into_qualified()
            .expect("refdb scan should always return qualified references")
    })
}

fn qualified_to_fullname(n: Qualified<'_>) -> FullName {
    let name = n.into_refstring().into_bstring();
    FullName::try_from(name).expect("`Namespaced` is a valid `FullName`")
}

fn namespaced_to_fullname(ns: Namespaced<'_>) -> FullName {
    qualified_to_fullname(ns.into_qualified())
}

fn fullname_to_refstring(name: FullName) -> Result<RefString, git_ext::ref_format::Error> {
    RefString::try_from(Vec::from(name.into_inner()).into_string_lossy())
}

fn force_reflog(ns: &Namespaced<'_>) -> bool {
    let refname = ns.strip_namespace();
    let (_refs, cat, _, _) = refname.non_empty_components();
    cat.as_str() == "rad"
}
