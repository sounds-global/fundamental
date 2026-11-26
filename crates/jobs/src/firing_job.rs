use chrono::{DateTime, Utc};

use crate::{JobKind, JobSubject};

/// A job being fired, as its handler sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiringJob<K: JobKind> {
    /// Which deadline.
    pub kind: K,
    /// The occurrence it belongs to.
    pub subject: JobSubject,
    /// When it was due.
    pub due_at: DateTime<Utc>,
    /// When it is being fired, by the database's clock — the instant its mark records.
    pub fired_at: DateTime<Utc>,
    /// How many earlier attempts failed.
    pub failed_attempts: i64,
}
