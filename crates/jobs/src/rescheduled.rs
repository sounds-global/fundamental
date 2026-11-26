/// What [`reschedule`](crate::ScheduleJobs::reschedule) did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Rescheduled {
    /// The pending job now waits for the new due time, with its attempts reset.
    Moved,
    /// No pending job of this kind and subject: never scheduled, cancelled, or already fired.
    NotPending,
}
