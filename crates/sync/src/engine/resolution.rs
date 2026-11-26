//! The outcome of resolving a sync conflict.

/// What to do when a row changed on both sides since the last sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Apply the remote version (overwrite local).
    TakeRemote,
    /// Keep the local version (discard the remote change).
    KeepLocal,
    /// The strategy cannot resolve this; the application must decide.
    Manual,
}
