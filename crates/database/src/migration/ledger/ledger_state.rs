//! [`LedgerState`]: where one migration on disk stands in a database.

use chrono::{DateTime, Utc};

use super::{FailedStep, Resolved};

/// Where one migration on disk stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerState {
    /// Every step is done and it is recorded, with the resolution that settled it, if one did.
    Applied {
        /// When it was recorded.
        at: DateTime<Utc>,
        /// The latest resolution of it.
        resolved: Option<Resolved>,
    },
    /// A step failed and nothing has settled it.
    Failed(FailedStep),
    /// A step was started and has not ended: a migrator is running it, or stopped before its
    /// answer arrived.
    Running {
        /// Which step.
        step: usize,
        /// Of how many.
        total: usize,
        /// Since when.
        since: DateTime<Utc>,
    },
    /// Not applied yet, and nothing before it is in the way.
    Pending,
    /// Not applied, and behind a migration that failed: `apply` reaches it once that one is
    /// settled.
    Blocked {
        /// The migration in the way.
        behind: String,
    },
}
