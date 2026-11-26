//! Database constraint kinds.

use core::fmt;
use fundamental_macros::ErrorOverride;

/// The kind of database constraint that was violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ErrorOverride)]
pub enum ConstraintKind {
    /// Unique constraint violation.
    #[overrides(status = 409)]
    Unique,
    /// Foreign key constraint violation.
    ForeignKey,
    /// Not null constraint violation.
    NotNull,
    /// Check constraint violation.
    Check,
}

impl fmt::Display for ConstraintKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unique => write!(f, "UNIQUE"),
            Self::ForeignKey => write!(f, "FOREIGN KEY"),
            Self::NotNull => write!(f, "NOT NULL"),
            Self::Check => write!(f, "CHECK"),
        }
    }
}
