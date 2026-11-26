//! [`Unheld`], what a ledger did not hold.

use super::PendingMismatch;

/// One thing a [`PendingLedger`](super::PendingLedger) did not hold, in the order a reader should
/// meet it: a mismatched entry is followed by the findings that made it one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unheld<F> {
    /// An entry whose count is not what was found.
    Mismatch(PendingMismatch),
    /// A finding no entry freezes.
    Finding(F),
}
