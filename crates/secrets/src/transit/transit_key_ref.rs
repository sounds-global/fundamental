//! [`TransitKeyRef`] — one version of one key in one Transit engine.

use std::num::NonZeroU32;

use crate::transit::{TransitKeyName, TransitMount};

/// A pinned reference to one version of a Transit key, so an out-of-band rotation cannot change the signer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitKeyRef {
    /// The engine's mount.
    pub mount: TransitMount,
    /// The key's name.
    pub name: TransitKeyName,
    /// The key version signatures are made with.
    pub version: NonZeroU32,
}

impl TransitKeyRef {
    /// A reference to `version` of `name` under `mount`.
    #[must_use]
    pub fn new(mount: TransitMount, name: TransitKeyName, version: NonZeroU32) -> Self {
        Self {
            mount,
            name,
            version,
        }
    }
}
