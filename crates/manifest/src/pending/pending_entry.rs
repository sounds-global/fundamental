//! [`PendingEntry`], one file's frozen findings of one rule.

use std::num::NonZeroU32;
use std::path::PathBuf;

/// One file's existing findings of one rule, an exact count frozen until `until` pays them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEntry<R> {
    /// The file, relative to the project.
    pub file: PathBuf,
    /// The rule it freezes.
    pub rule: R,
    /// What the findings reach or sit in, for a rule with a name key; `None` otherwise.
    pub name: Option<String>,
    /// How many findings of that rule the file holds.
    pub count: NonZeroU32,
    /// The work that pays them.
    pub until: String,
}
