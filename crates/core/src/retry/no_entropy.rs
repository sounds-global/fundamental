//! [`NoEntropy`] — an [`Entropy`] source that has nothing to give.

use super::Entropy;

/// Always empty, so every draw is its window's upper bound (the `seeded-entropy` feature).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoEntropy;

impl Entropy for NoEntropy {
    fn next_u64(&mut self) -> Option<u64> {
        None
    }
}
