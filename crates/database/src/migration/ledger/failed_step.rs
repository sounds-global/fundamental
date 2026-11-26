//! [`FailedStep`]: where a failed migration stopped.

use chrono::{DateTime, Utc};

/// The step a migration failed at, with the statement from its file and the engine's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStep {
    /// Which step, counting from one.
    pub step: usize,
    /// How many steps the migration has.
    pub total: usize,
    /// The statement, as the file on disk has it now.
    pub statement: String,
    /// The engine's code for the failure — the SQLSTATE on YSQL — when it gave one.
    pub code: Option<String>,
    /// The engine's message, or what is known when the answer was lost.
    pub error: String,
    /// Whether the answer was lost rather than given, so whether it applied is unknown.
    pub answer_lost: bool,
    /// When it failed.
    pub at: DateTime<Utc>,
}
