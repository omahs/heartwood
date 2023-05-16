use std::str::FromStr;

use either::Either;
use git_ext::ref_format::{qualified, Component, Namespaced, Qualified, RefString};
use gix_hash::ObjectId;
use gix_object::bstr::{BStr, BString, ByteSlice};
use once_cell::sync::Lazy;
use radicle_crypto::PublicKey;
use thiserror::Error;

use crate::gix::refdb::{Policy, Update};

pub(crate) static REFS_RAD_ID: Lazy<Qualified<'static>> = Lazy::new(|| qualified!("refs/rad/id"));
pub(crate) static REFS_RAD_SIGREFS: Lazy<Qualified<'static>> =
    Lazy::new(|| qualified!("refs/rad/sigrefs"));

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("ref name '{0}' is not qualified")]
    NotQualified(RefString),

    #[error("ref name '{0}' is not namespaced")]
    NotNamespaced(Qualified<'static>),

    #[error("invalid remote peer id")]
    PublicKey(#[from] radicle_crypto::PublicKeyError),

    #[error("malformed ref name")]
    Check(#[from] git_ext::ref_format::Error),

    #[error("malformed ref name")]
    MalformedSuffix,

    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),
}

#[derive(Clone, Copy, Debug)]
pub enum Special {
    /// `rad/id`
    Id,
    /// `rad/sigrefs`
    SignedRefs,
}

impl From<Special> for Qualified<'_> {
    fn from(s: Special) -> Self {
        match s {
            Special::Id => (*REFS_RAD_ID).clone(),
            Special::SignedRefs => (*REFS_RAD_SIGREFS).clone(),
        }
    }
}

/// A reference name under a `remote` namespace.
///
/// # Examples
///
///   * `refs/namespaces/<remote>/refs/rad/id`
///   * `refs/namespaces/<remote>/refs/rad/sigrefs`
///   * `refs/namespaces/<remote>/refs/heads/main`
///   * `refs/namespaces/<remote>/refs/cobs/issue.rad.xyz`
#[derive(Debug)]
pub struct Refname<'a> {
    pub remote: PublicKey,
    pub suffix: Either<Special, Qualified<'a>>,
}

impl<'a> Refname<'a> {
    pub fn is_special(&self) -> bool {
        self.suffix.is_left()
    }

    pub fn remote(remote: PublicKey, suffix: Qualified<'a>) -> Self {
        Self {
            remote,
            suffix: Either::Right(suffix),
        }
    }

    pub fn rad_id<'b>(remote: PublicKey) -> Namespaced<'b> {
        Self {
            remote,
            suffix: Either::Left(Special::Id),
        }
        .namespaced()
    }

    pub fn rad_sigrefs<'b>(remote: PublicKey) -> Namespaced<'b> {
        Self {
            remote,
            suffix: Either::Left(Special::SignedRefs),
        }
        .namespaced()
    }

    pub fn to_qualified<'b>(&self) -> Qualified<'b> {
        match &self.suffix {
            Either::Left(s) => (*s).into(),
            Either::Right(name) => name.clone().into_owned(),
        }
    }

    pub fn to_ref_string(&self) -> RefString {
        self.to_qualified().into_refstring()
    }

    // TODO: rename to `to_namespaced`
    pub fn namespaced<'b>(&self) -> Namespaced<'b> {
        let ns = Component::from(&self.remote);
        self.to_qualified().with_namespace(ns)
    }
}

impl TryFrom<BString> for Refname<'_> {
    type Error = Error;

    fn try_from(value: BString) -> Result<Self, Self::Error> {
        let name = RefString::try_from(value.to_str()?)?;
        Self::try_from(name)
    }
}

impl TryFrom<&BStr> for Refname<'_> {
    type Error = Error;

    fn try_from(value: &BStr) -> Result<Self, Self::Error> {
        let name = RefString::try_from(value.to_str()?)?;
        Self::try_from(name)
    }
}

impl TryFrom<RefString> for Refname<'_> {
    type Error = Error;

    fn try_from(r: RefString) -> Result<Self, Self::Error> {
        r.clone()
            .into_qualified()
            .ok_or(Error::NotQualified(r))
            .and_then(Self::try_from)
    }
}

impl<'a> TryFrom<Qualified<'a>> for Refname<'_> {
    type Error = Error;

    fn try_from(name: Qualified<'a>) -> Result<Self, Self::Error> {
        fn parse_suffix<'a>(
            head: Component<'a>,
            mut iter: impl Iterator<Item = Component<'a>>,
        ) -> Option<Special> {
            match (head.as_str(), iter.next()) {
                ("id", None) => Some(Special::Id),
                ("sigrefs", None) => Some(Special::SignedRefs),
                _ => None,
            }
        }

        let ns = name.clone();
        let ns = ns
            .to_namespaced()
            .ok_or_else(|| Error::NotNamespaced(name.to_owned()))?;
        let remote = PublicKey::from_str(ns.namespace().as_str())?;
        let name = ns.strip_namespace();
        let suffix = match name.non_empty_components() {
            (_refs, cat, head, tail) if "rad" == cat.as_str() => {
                parse_suffix(head, tail).map(Either::Left)
            }
            _ => Some(Either::Right(name)),
        };
        Ok(Refname {
            remote,
            suffix: suffix.ok_or(Error::MalformedSuffix)?,
        })
    }
}

#[derive(Debug)]
pub struct ReceivedRef {
    pub tip: ObjectId,
    pub name: Refname<'static>,
}

impl ReceivedRef {
    pub fn new(tip: ObjectId, name: Refname<'static>) -> Self {
        Self { tip, name }
    }

    // TODO: change docs
    /// If [`Self`] is a ref needed for verification, convert to an appropriate
    /// [`Update`].
    ///
    /// A verification ref is a [`refs::parsed::Rad`] ref, except for the
    /// [`refs::parsed::Rad::Selv`] variant which needs to be handled
    /// separately.
    pub fn as_verification_ref_update(&self) -> Option<Update<'static>> {
        self.name
            .suffix
            .as_ref()
            .left()
            .map(|special| match special {
                Special::Id | Special::SignedRefs => Update::Direct {
                    name: self.name.namespaced(),
                    target: self.tip,
                    no_ff: Policy::Abort,
                },
            })
    }

    pub fn remote(&self) -> &PublicKey {
        &self.name.remote
    }
}
