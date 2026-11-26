//! [`Verdict`] — what became of one delivery's transaction.

use crate::ParkReason;

/// What one attempt at a delivery came to, before it is turned into a settlement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The effect committed with its inbox row.
    Processed,
    /// The row was already there with the same payload: the effect committed before.
    Duplicate,
    /// The delivery is refused for what it is; retrying cannot change that.
    Refused { reason: ParkReason, detail: String },
    /// The payload does not decode. A replica running an older version meets a newer event
    /// this way during a deploy, so it is delivered again until the budget is spent.
    Undecodable { detail: String },
    /// The attempt failed and rolled back.
    Failed { code: String, retryable: bool },
    /// The commit was sent and its answer lost.
    CommitUnknown,
    /// The handler panicked; the transaction rolled back.
    Panicked,
    /// The effect outlived its deadline and was abandoned.
    DeadlineExceeded,
}

impl Verdict {
    /// The stable name of the outcome, for the span and the log.
    pub(crate) fn outcome(&self) -> &'static str {
        match self {
            Self::Processed => "processed",
            Self::Duplicate => "duplicate",
            Self::Refused { .. } => "refused",
            Self::Undecodable { .. } => "undecodable",
            Self::Failed { .. } => "failed",
            Self::CommitUnknown => "commit_unknown",
            Self::Panicked => "panicked",
            Self::DeadlineExceeded => "deadline_exceeded",
        }
    }
}
