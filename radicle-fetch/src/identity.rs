use git_ext::ref_format::Component;
use gix_hash::ObjectId;
use nonempty::NonEmpty;
use radicle_crypto::PublicKey;
use thiserror::Error;

use crate::gix::{refdb, Refdb};
use crate::refs;

#[derive(Debug, Error)]
pub enum Error<E: std::error::Error + Send + Sync + 'static> {
    #[error(transparent)]
    Find(#[from] refdb::error::Find),

    #[error(transparent)]
    Verified(E),
}

pub trait Verified {
    fn delegates(&self) -> NonEmpty<PublicKey>;
}

pub trait Identities {
    type VerifiedIdentity: Verified;

    type VerifiedError: std::error::Error + Send + Sync + 'static;

    fn verified(&self, head: ObjectId) -> Result<Self::VerifiedIdentity, Self::VerifiedError>;
}

pub fn current<I>(
    local: &PublicKey,
    ids: &I,
    refdb: &Refdb,
) -> Result<Option<I::VerifiedIdentity>, Error<I::VerifiedError>>
where
    I: Identities,
{
    let rad_id = refs::REFS_RAD_ID.with_namespace(Component::from(local));
    refdb
        .refname_to_id(rad_id)?
        .map(|tip| ids.verified(tip).map_err(Error::Verified))
        .transpose()
}
