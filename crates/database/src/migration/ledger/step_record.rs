//! [`StepRecord`]: one step of a migration as the ledger holds it.

use chrono::{DateTime, Utc};

/// Where one step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Recorded before its statement was sent, and nothing since: a migrator is running it, or
    /// stopped before its answer arrived.
    Running,
    /// Its statement applied.
    Done,
    /// Its statement failed, with the engine's answer or with the answer lost.
    Failed,
}

impl StepState {
    /// The word the ledger stores it as.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// The state `word` stores, or `None` for a word the ledger never writes.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// One row of the step table: a step of a migration, the hash of its statement, where it
/// stands, and when it got there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// The migration's ledger name.
    pub name: String,
    /// Which step, counting from one.
    pub step: usize,
    /// How many steps the migration has.
    pub total: usize,
    /// The content hash of the statement it ran, so a step is skipped only while its file is
    /// unchanged.
    pub statement_hash: String,
    /// Where it stands.
    pub state: StepState,
    /// The engine's code for the failure — the SQLSTATE on YSQL — when it gave one.
    pub code: Option<String>,
    /// The engine's message, for a failed step.
    pub error: Option<String>,
    /// Whether the answer was lost, so whether the statement applied is unknown.
    pub answer_lost: bool,
    /// When it got to its state.
    pub at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::StepState;

    #[test]
    fn each_state_reads_back_from_its_word() {
        for state in [StepState::Running, StepState::Done, StepState::Failed] {
            assert_eq!(StepState::parse(state.as_str()), Some(state));
        }
        assert_eq!(StepState::parse("retrying"), None);
    }
}
