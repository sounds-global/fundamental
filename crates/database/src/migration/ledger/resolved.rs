//! [`Resolved`]: a resolution act as the ledger recorded it.

use chrono::{DateTime, Utc};

use super::Resolve;

/// One recorded `migrate resolve`: the act, who and why, the database login it ran as, and the
/// failure it settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The migration's ledger name.
    pub name: String,
    /// The act.
    pub act: Resolve,
    /// Who settled it.
    pub by: String,
    /// Why.
    pub reason: String,
    /// The database login the act ran as, where the engine names one.
    pub login: Option<String>,
    /// The step it settled.
    pub step: usize,
    /// The engine's code for the failure it settled, when there was one.
    pub code: Option<String>,
    /// When.
    pub at: DateTime<Utc>,
}
