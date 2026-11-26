//! [`PackedFile`] — one file a package holds, as its `package.json` names it.

use serde::{Deserialize, Serialize};

use crate::{FileMode, PackedPath};

/// One file a package holds, as its `package.json` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackedFile {
    /// Where the package holds it.
    pub path: PackedPath,
    /// Its bytes.
    pub size: u64,
    /// Whether it runs.
    pub mode: FileMode,
    /// The BLAKE3 of its bytes, as 64 lowercase hexadecimal digits.
    pub blake3: String,
}
