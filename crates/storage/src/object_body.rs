//! What a read answers.

use crate::ByteStream;
use std::fmt;

/// The bytes of an object, or of a range of it, as they arrive.
pub struct ObjectBody {
    /// How many bytes the stream yields.
    pub len: u64,
    /// The whole object's length, whatever range was read.
    pub object_len: u64,
    /// The bytes.
    pub stream: ByteStream,
}

impl fmt::Debug for ObjectBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectBody")
            .field("len", &self.len)
            .field("object_len", &self.object_len)
            .finish_non_exhaustive()
    }
}
