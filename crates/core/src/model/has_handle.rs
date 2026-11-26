//! [`HasHandle`] — an identifier with a public spelling.

use super::{Bits128, HandlePrefix, Identifier};

/// An identifier whose public spelling is a [`Handle`](super::Handle), declared with `#[identifier(handle = "…")]`.
pub trait HasHandle: Identifier<Inner: Bits128> {
    /// The kind's prefix.
    const HANDLE_PREFIX: HandlePrefix;
}
