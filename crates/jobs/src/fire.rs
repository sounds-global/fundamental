use chrono::{DateTime, Utc};

/// What a [`JobHandler`](crate::JobHandler) decided about the job it was asked to fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Fire {
    /// The effect committed; the job is fired and never armed again.
    Done,
    /// Due again at this instant, its attempts reset, as
    /// [`reschedule`](crate::ScheduleJobs::reschedule) leaves it.
    ///
    /// An instant not after [`FiringJob::fired_at`](crate::FiringJob::fired_at), or outside the
    /// years 2000 to 2200, fails the attempt with
    /// [`JobError::PostponeRefused`](crate::JobError::PostponeRefused).
    NotBefore(DateTime<Utc>),
    /// Not yet decidable: asked again one `[jobs] defer_secs` later, keeping its due time. The
    /// handler's writes commit on every deferral, so they must be idempotent.
    Defer,
}
