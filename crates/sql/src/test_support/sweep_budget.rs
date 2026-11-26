use std::time::Duration;

/// How much one schema sweep may do: how many schemas it drops, and for how long; listing is never capped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepBudget {
    /// The most schemas one sweep removes.
    pub drops: usize,
    /// How long the whole sweep, connection and release included, may take.
    pub time: Duration,
}

impl SweepBudget {
    /// For a sweep on the way into a fixture: sixteen drops, ten seconds, so it never becomes a catalog storm.
    pub const ON_PROVISION: Self = Self {
        drops: 16,
        time: Duration::from_secs(10),
    };

    /// For the once-per-run rite across every database it sweeps, kept far inside nextest's 300 s.
    pub const ONCE_PER_RUN: Self = Self {
        drops: 128,
        time: Duration::from_secs(60),
    };
}
