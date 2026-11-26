//! What a put answers.

use crate::ShardIndex;

/// Where an object went and how large it is — what its caller records beside its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredObject {
    /// The store, within its bucket, that holds it.
    pub shard: ShardIndex,
    /// The bytes the caller handed over: the plaintext, for a private object.
    pub content_len: u64,
    /// The bytes the provider holds.
    pub stored_len: u64,
}
