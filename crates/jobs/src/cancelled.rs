/// What [`cancel`](crate::ScheduleJobs::cancel) did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Cancelled {
    /// The pending job is gone and will not fire.
    Removed,
    /// No pending job of this kind and subject: never scheduled, cancelled, or already fired.
    NotPending,
}
