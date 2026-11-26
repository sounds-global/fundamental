use chrono::{DateTime, Utc};

/// What [`schedule`](crate::ScheduleJobs::schedule) did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Scheduled {
    /// A new deadline: no job of this kind existed for the subject.
    Created,
    /// The pending job was moved from `from` to the requested due time.
    Moved {
        /// The due time it had.
        from: DateTime<Utc>,
    },
    /// The job was already pending at exactly this due time.
    Unchanged,
    /// The job already fired, at `fired_at`; it is never armed or fired again.
    AlreadyFired {
        /// When it fired, by the database's clock.
        fired_at: DateTime<Utc>,
    },
}
