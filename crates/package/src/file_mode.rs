//! [`FileMode`] — whether a packed file runs.

use serde::{Deserialize, Serialize};

/// Whether a packed file runs: the only two modes a package carries, so no unpacked file is
/// writable by others, set-uid or a special file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMode {
    /// Read by all, written by its owner (`0644`).
    Plain,
    /// A program: read and run by all, written by its owner (`0755`).
    Executable,
}

impl FileMode {
    /// The unix permission bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        match self {
            Self::Plain => 0o644,
            Self::Executable => 0o755,
        }
    }
}
