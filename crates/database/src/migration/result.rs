//! Migration result alias.

use super::MigrationError;

/// Result type for migration operations.
pub type MigrationResult<T> = Result<T, MigrationError>;
