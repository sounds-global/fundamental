//! [`SweepReport`] — what one sweep removed.

/// What one pass of the sweep removed, across every effect this inbox listens for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Effects swept.
    pub effects: usize,
    /// Inbox rows past the retention window, deleted.
    pub deleted: u64,
    /// Parked records past their window, deleted.
    pub parked_deleted: u64,
    /// Bounded statements run.
    pub batches: u64,
    /// Whether the time budget ran out while there was still more to delete.
    pub behind: bool,
}
