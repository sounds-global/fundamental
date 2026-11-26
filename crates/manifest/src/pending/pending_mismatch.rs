//! [`PendingMismatch`], an entry the findings no longer match.

/// A ledger entry whose count is not what the check found: debt paid and not given back, new
/// debt in a frozen file, or an entry for a file that is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMismatch {
    /// Where the ledger is written, as `path:line`.
    pub at: String,
    /// The entry's file.
    pub file: String,
    /// The entry's rule code.
    pub rule: &'static str,
    /// The entry's name, when its rule has a name key.
    pub name: Option<String>,
    /// The count the entry freezes.
    pub frozen: u32,
    /// What the check found instead.
    pub found: String,
    /// What to do about it.
    pub advice: String,
}
