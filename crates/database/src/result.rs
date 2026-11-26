//! Result alias for database operations.

use super::DatabaseError;

/// Convenience alias for results returned by database operations.
pub type DatabaseResult<T> = Result<T, DatabaseError>;
