use super::SqlError;

/// Result type for YSQL operations.
pub type SqlResult<T> = Result<T, SqlError>;
