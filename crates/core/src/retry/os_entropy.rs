//! [`OsEntropy`] — the operating system's CSPRNG as an [`Entropy`] source.

use tracing::warn;

use super::Entropy;

/// Draws from the OS CSPRNG through `getrandom`, so a fleet's return cannot be predicted.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn next_u64(&mut self) -> Option<u64> {
        let mut bytes = [0u8; 8];
        match getrandom::getrandom(&mut bytes) {
            Ok(()) => Some(u64::from_le_bytes(bytes)),
            Err(error) => {
                warn!(%error, "backoff: no entropy for jitter; waiting the window's upper bound");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_distinct_words() {
        let mut entropy = OsEntropy;
        let first = entropy.next_u64();
        let second = entropy.next_u64();
        assert!(first.is_some(), "the test host has a CSPRNG");
        assert_ne!(
            first, second,
            "two CSPRNG words colliding is a broken source"
        );
    }
}
