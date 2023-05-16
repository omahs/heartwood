pub mod odb;
pub use odb::Odb;

pub mod refdb;
pub use refdb::Refdb;

pub use bstr::BString;
pub use gix_hash::ObjectId;

pub mod oid {
    use super::ObjectId;

    pub fn to_oid(oid: ObjectId) -> git_ext::Oid {
        git_ext::Oid::try_from(oid.as_bytes()).expect("invalid gix Oid")
    }

    pub fn to_object_id(oid: git_ext::Oid) -> ObjectId {
        ObjectId::try_from(oid.as_bytes()).expect("invalid git-ext Oid")
    }
}
