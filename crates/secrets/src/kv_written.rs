//! [`KvWritten`] — what a key-value write produced.

use crate::KvVersion;

/// The outcome of a key-value (version 2) write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct KvWritten {
    /// The version the write created.
    pub version: KvVersion,
}
