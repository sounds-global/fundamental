//! [`Entropy`] — the random source a [`Backoff`](super::Backoff) draws its jitter from.

/// A source of uniformly random 64-bit words.
///
/// A client never accepts one from its caller: a source that always answers `Some(0)` is a zero wait.
pub trait Entropy {
    /// The next random word, or `None` when the source has nothing to give.
    fn next_u64(&mut self) -> Option<u64>;
}
