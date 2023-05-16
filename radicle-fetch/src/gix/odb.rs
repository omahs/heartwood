use std::path::Path;

use gix_hash as hash;
use gix_hash::ObjectId;
use gix_object::{CommitRefIter, Kind};
use gix_odb::loose::{find, Store};
use gix_traverse::commit;

pub mod error {
    use gix_hash::ObjectId;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Revwalk {
        #[error(transparent)]
        Find(#[from] gix_odb::loose::find::Error),
        #[error("missing object {0}")]
        MissingObject(ObjectId),
        #[error(transparent)]
        Traverse(#[from] gix_traverse::commit::ancestors::Error),
    }
}

pub struct Odb {
    inner: Store,
}

pub struct Object<'a> {
    pub kind: Kind,
    pub data: &'a [u8],
}

impl<'a> From<gix_object::Data<'a>> for Object<'a> {
    fn from(data: gix_object::Data<'a>) -> Self {
        Self {
            kind: data.kind,
            data: data.data,
        }
    }
}

impl Odb {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            inner: Store::at(dir.as_ref().to_path_buf(), hash::Kind::Sha1),
        }
    }

    pub fn contains(&self, oid: impl AsRef<hash::oid>) -> bool {
        self.inner.contains(oid)
    }

    pub fn lookup<'a>(
        &'a self,
        oid: impl AsRef<hash::oid>,
        out: &'a mut Vec<u8>,
    ) -> Result<Option<Object>, find::Error> {
        self.inner
            .try_find(oid, out)
            .map(|obj| obj.map(Object::from))
    }

    pub fn is_in_ancestry_path(
        &self,
        new: impl Into<ObjectId>,
        old: impl Into<ObjectId>,
    ) -> Result<bool, error::Revwalk> {
        let new = new.into();
        let old = old.into();

        if new == old {
            return Ok(true);
        }

        if !self.inner.contains(new) || !self.inner.contains(old) {
            return Ok(false);
        }

        let revwalk = commit::Ancestors::new(
            Some(new),
            commit::ancestors::State::default(),
            move |oid, buf| -> Result<CommitRefIter, error::Revwalk> {
                let obj = self
                    .try_find(oid, buf)?
                    .ok_or_else(|| error::Revwalk::MissingObject(oid.into()))?;
                Ok(CommitRefIter::from_bytes(obj.data))
            },
        );

        for parent in revwalk {
            if parent? == old {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn try_find<'a>(
        &self,
        id: &hash::oid,
        out: &'a mut Vec<u8>,
    ) -> Result<Option<Object<'a>>, find::Error> {
        let data = self.inner.try_find(id, out)?;
        data.map(|data| {
            Ok(Object {
                kind: data.kind,
                data: data.data,
            })
        })
        .transpose()
    }
}
