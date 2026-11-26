/// What one schema sweep did, counted; a sweep never fails its caller, so this is all it reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Held schemas removed because no session bore their name and their lock was free.
    pub reclaimed_dead: usize,
    /// Unmarked ephemeral schemas removed because they were older than the age bound.
    pub reclaimed_aged: usize,
    /// Held schemas left alone because their run is alive.
    pub live: usize,
    /// Held schemas left alone because liveness could not be decided.
    pub unknown: usize,
    /// Another sweep was already running in this database, so this one did nothing.
    pub busy: bool,
    /// The drop budget ran out with candidates left; the next sweep takes them.
    pub capped: bool,
    /// The time budget ran out; the counts are what was done before it did.
    pub timed_out: bool,
}

impl SweepOutcome {
    /// How many schemas this sweep removed, by either rule.
    #[must_use]
    pub fn reclaimed(&self) -> usize {
        self.reclaimed_dead + self.reclaimed_aged
    }
}
