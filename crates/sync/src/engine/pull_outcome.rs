//! The result of a [`pull`](crate::SyncEngine::pull).

/// A summary of what a pull did, for logging and callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PullOutcome {
    /// Remote rows applied to the local mirror (new rows and conflict winners).
    pub applied: usize,
    /// Remote rows discarded because the local version won the conflict.
    pub kept_local: usize,
    /// Conflicts left for the application to resolve.
    pub conflicts: usize,
}
