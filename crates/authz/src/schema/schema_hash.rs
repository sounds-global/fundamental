//! [`SchemaHash`]: a stable content hash of a compiled schema.

use core::fmt;

use serde::{Deserialize, Serialize};

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A stable, deterministic fingerprint of a compiled [`SchemaIr`](super::SchemaIr).
///
/// FNV-1a over the schema's canonical bytes, stable across processes and platforms, so it
/// can travel inside a zookie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SchemaHash(u64);

impl SchemaHash {
    /// The hash of an empty schema (the FNV-1a offset basis).
    #[must_use]
    pub const fn empty() -> Self {
        Self(FNV_OFFSET)
    }

    /// Returns the raw 64-bit value.
    #[must_use]
    pub const fn to_u64(self) -> u64 {
        self.0
    }

    /// Folds `bytes` into a running hash, returning the updated value.
    #[must_use]
    pub fn update(self, bytes: &[u8]) -> Self {
        let mut hash = self.0;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        Self(hash)
    }
}

impl From<u64> for SchemaHash {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Display for SchemaHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic_and_order_sensitive() {
        let a = SchemaHash::empty().update(b"document").update(b"viewer");
        let b = SchemaHash::empty().update(b"document").update(b"viewer");
        assert_eq!(a, b, "same input must hash identically");

        let swapped = SchemaHash::empty().update(b"viewer").update(b"document");
        assert_ne!(a, swapped, "order must affect the hash");
    }

    #[test]
    fn empty_differs_from_populated() {
        assert_ne!(SchemaHash::empty(), SchemaHash::empty().update(b"x"));
    }

    #[test]
    fn renders_as_padded_hex() {
        assert_eq!(SchemaHash::from(0x1f).to_string(), "000000000000001f");
    }
}
