//! [`KvCas`] — what a key-value write requires of the version it replaces.

use crate::KvVersion;

/// The check-and-set a key-value (version 2) write carries; deliberately without a default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KvCas {
    /// Write whatever the current version is.
    Unchecked,
    /// Write only if the secret has no version yet.
    Absent,
    /// Write only if the current version is exactly this one.
    At(KvVersion),
}

impl KvCas {
    /// The engine's `options.cas`, or `None` when the write is unchecked.
    pub(crate) const fn option(self) -> Option<u64> {
        match self {
            Self::Unchecked => None,
            Self::Absent => Some(0),
            Self::At(version) => Some(version.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_is_the_engines_zero_and_unchecked_sends_no_option() {
        assert_eq!(KvCas::Unchecked.option(), None);
        assert_eq!(KvCas::Absent.option(), Some(0));
        let at = KvVersion::new(4).map(KvCas::At);
        assert_eq!(at.and_then(KvCas::option), Some(4));
    }
}
