//! [`KvVersion`] — one version of a key-value (version 2) secret.

use std::fmt;
use std::num::NonZeroU64;

/// A version of a key-value (version 2) secret, numbered from 1 (0 means absent to the engine).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KvVersion(NonZeroU64);

impl KvVersion {
    /// The version numbered `n`, or `None` for 0.
    #[must_use]
    pub const fn new(n: u64) -> Option<Self> {
        match NonZeroU64::new(n) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }

    /// The version's number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for KvVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::KvVersion;

    #[test]
    fn zero_is_not_a_version() {
        assert_eq!(KvVersion::new(0), None);
        assert_eq!(KvVersion::new(7).map(KvVersion::get), Some(7));
    }
}
