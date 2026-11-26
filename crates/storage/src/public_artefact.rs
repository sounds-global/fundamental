//! Content meant for anyone with a link.

use crate::ByteStream;
use std::fmt;

/// A release artefact, such as a signed build, that a [`Public`](crate::Public) bucket stores as it is.
pub struct PublicArtefact {
    pub(crate) max_len: u64,
    pub(crate) stream: ByteStream,
}

impl PublicArtefact {
    /// The artefact `stream`, refused if it runs past `max_len` bytes.
    #[must_use]
    pub fn new(max_len: u64, stream: ByteStream) -> Self {
        Self { max_len, stream }
    }
}

impl fmt::Debug for PublicArtefact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicArtefact")
            .field("max_len", &self.max_len)
            .finish_non_exhaustive()
    }
}
