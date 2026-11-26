//! [`SeededEntropy`] — a deterministic [`Entropy`] source for fleet tests.

use super::Entropy;

/// SplitMix64 from a fixed seed: uniform, and identical on every run.
///
/// Built only for tests (the `seeded-entropy` feature); no client accepts a source from its caller.
#[derive(Debug, Clone)]
pub struct SeededEntropy {
    state: u64,
}

impl SeededEntropy {
    /// A source seeded with `seed`; one seed per simulated client.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl Entropy for SeededEntropy {
    fn next_u64(&mut self) -> Option<u64> {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Some(z ^ (z >> 31))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_seed_answers_the_same_way_every_run() {
        let mut a = SeededEntropy::new(7);
        let mut b = SeededEntropy::new(7);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(
            SeededEntropy::new(7).next_u64(),
            SeededEntropy::new(8).next_u64()
        );
    }
}
