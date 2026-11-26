//! Object storage behind a provider port: an [`ObjectStore`] reached only through a [`Bucket`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bucket;
mod byte_range;
mod byte_stream;
#[cfg(feature = "conformance")]
mod conformance;
#[cfg(feature = "fs")]
mod fs_object_store;
#[cfg(feature = "fake")]
mod memory_object_store;
mod object_body;
mod object_key;
mod object_store;
mod private;
mod public;
mod public_artefact;
mod shard_index;
mod store_error;
mod stored_object;
mod visibility;

pub use bucket::Bucket;
pub use byte_range::ByteRange;
pub use byte_stream::ByteStream;
#[cfg(feature = "conformance")]
pub use conformance::Conformance;
#[cfg(feature = "fs")]
pub use fs_object_store::FsObjectStore;
#[cfg(feature = "fake")]
pub use memory_object_store::MemoryObjectStore;
pub use object_body::ObjectBody;
pub use object_key::ObjectKey;
pub use object_store::ObjectStore;
pub use private::Private;
pub use public::Public;
pub use public_artefact::PublicArtefact;
pub use shard_index::ShardIndex;
pub use store_error::StoreError;
pub use stored_object::StoredObject;
pub use visibility::Visibility;
