//! Safety classification for schema changes.

use std::fmt;

use super::SchemaChange;

/// Safety tier for migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SafetyTier {
    /// Safe changes that can be auto-applied.
    ///
    /// For example: add a nullable column, an index, a table.
    Safe,
    /// Changes that require review before applying.
    ///
    /// For example: add a non-null column with a default, drop an index.
    Review,
    /// Destructive changes that require explicit confirmation.
    ///
    /// For example: drop a column or table, rename a column, change a type.
    Destructive,
}

impl fmt::Display for SafetyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Safe => "Safe",
            Self::Review => "Review Required",
            Self::Destructive => "Destructive",
        })
    }
}

impl SafetyTier {
    /// Determines the safety tier from a list of schema changes.
    pub fn from_changes(changes: &[SchemaChange]) -> Self {
        changes
            .iter()
            .map(|c| c.safety_tier())
            .max()
            .unwrap_or(SafetyTier::Safe)
    }

    /// Returns an emoji indicator.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Safe => "✅",
            Self::Review => "⚠️",
            Self::Destructive => "🔴",
        }
    }
}
