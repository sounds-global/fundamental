//! [`ClockScope`]: a read scope that carries an instant and nothing else.

use super::{ReadScope, WallTime};
use crate::zookie::{HybridClock, HybridTs};

/// A scope for a backend whose coherence does not come from a transaction.
///
/// For the wide-column store and the offline mirror, which have no snapshot to pin. Its `now`
/// is the process's wall clock.
#[derive(Debug, Clone, Copy)]
pub struct ClockScope {
    ts: HybridTs,
    now: WallTime,
}

impl ClockScope {
    /// A scope observing `ts`, whose caveats read `now` as the time.
    #[must_use]
    pub fn new(ts: HybridTs, now: WallTime) -> Self {
        Self { ts, now }
    }

    /// A scope opened now: its instant from the [`HybridClock`], its `now` from the wall clock.
    #[must_use]
    pub fn current() -> Self {
        Self::new(HybridClock::now(), WallTime::now())
    }
}

impl ReadScope for ClockScope {
    fn ts(&self) -> HybridTs {
        self.ts
    }

    fn now(&self) -> WallTime {
        self.now
    }
}
