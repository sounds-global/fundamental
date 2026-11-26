use chrono::{DateTime, Utc};

/// What [`hold`](crate::ScheduleJobs::hold) found, by the database's clock once the row was locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Held {
    /// The pending job, held, and not yet due at `at`: it cannot fire before the transaction ends.
    Pending {
        /// When it falls due.
        due_at: DateTime<Utc>,
        /// The database's time once it was held.
        at: DateTime<Utc>,
    },
    /// The pending job, held, but due at `at`: a relay fires it once the transaction ends.
    Due {
        /// When it fell due.
        due_at: DateTime<Utc>,
        /// The database's time once it was held.
        at: DateTime<Utc>,
    },
    /// No pending job of this kind and subject: never scheduled, cancelled, or already fired.
    NotPending {
        /// The database's time when that was read.
        at: DateTime<Utc>,
    },
}

impl Held {
    /// The database's time the answer was judged at.
    pub const fn at(&self) -> DateTime<Utc> {
        match *self {
            Self::Pending { at, .. } | Self::Due { at, .. } | Self::NotPending { at } => at,
        }
    }
}
