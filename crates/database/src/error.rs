//! Database error type.

use core::fmt;
use fundamental_core::AnyError;
use fundamental_macros::Error;

use super::ConstraintKind;
use crate::migration::MigrationError;

/// Unified error type for database operations.
#[derive(Debug, Error)]
#[error_into(MigrationError)]
#[non_exhaustive]
pub enum DatabaseError {
    /// An error from a lower layer with no more specific variant.
    Other(AnyError),
    /// Connection error (failed to connect or connection lost).
    #[error(
        code = "DB_001",
        message = "Something went wrong while connecting to the database.",
        developer_message = "Connection error: {message}",
        status = 500
    )]
    Connection {
        /// Error message.
        message: String,
        /// Whether the error is retryable.
        retryable: bool,
    },

    /// Query execution error.
    #[error(
        code = "DB_002",
        message = "Something went wrong while executing the query.",
        developer_message = "Query error: {message}",
        status = 500
    )]
    Query {
        /// Error message.
        message: String,
        /// The query that failed (if available).
        query: Option<String>,
    },

    /// Constraint violation (unique, foreign key, check, etc.).
    #[error(
        code = "DB_003",
        message = "Something went wrong while executing the query.",
        developer_message = "Constraint violation: {constraint}",
        status = 400
    )]
    ConstraintViolation {
        /// The constraint that was violated.
        constraint: String,
        /// The type of constraint. A `Unique` kind refines the status to 409.
        #[overrides]
        kind: ConstraintKind,
    },

    /// Record not found.
    #[error(
        code = "DB_004",
        message = "Record not found: {entity} not found: {id}",
        developer_message = "{entity} not found: {id}",
        status = 404
    )]
    NotFound {
        /// The entity type.
        entity: String,
        /// The ID that was not found.
        id: String,
    },

    /// Transaction error.
    #[error(
        code = "DB_005",
        message = "Something went wrong while executing the transaction.",
        developer_message = "Transaction error: {message}",
        status = 500
    )]
    Transaction {
        /// Error message.
        message: String,
        /// Whether the transaction was rolled back.
        rolled_back: bool,
    },

    /// Serialization/deserialization error.
    #[error(
        code = "DB_006",
        message = "Something went wrong while serializing or deserializing data.",
        developer_message = "Serialization error: {message}",
        status = 500
    )]
    Serialization {
        /// Error message.
        message: String,
    },

    /// The pool could not serve a connection within its declared acquire bound.
    ///
    /// Carries no `Retry-After`: a pool has no honest delay to give, and its fill state is not
    /// told to the caller.
    #[error(
        code = "DB_007",
        message = "The service is at capacity. Try again shortly.",
        developer_message = "connection pool `{pool}` refused: no connection within {waited_ms}ms",
        status = 503,
        retryable
    )]
    PoolExhausted {
        /// Which pool refused, such as `sql.layer0`.
        pool: String,
        /// The acquire bound waited out, in milliseconds.
        waited_ms: u64,
    },

    /// Query timeout.
    #[error(
        code = "DB_008",
        message = "Query timeout after {timeout_ms}ms",
        developer_message = "Query timeout after {timeout_ms}ms",
        status = 503
    )]
    Timeout {
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// Invalid configuration.
    #[error(
        code = "DB_009",
        message = "Something went wrong while trying to configure the database.",
        developer_message = "Configuration error: {message}",
        status = 500
    )]
    Configuration {
        /// Error message.
        message: String,
    },

    /// Migration error.
    #[error(
        code = "DB_010",
        message = "Something went wrong while trying to run a database migration.",
        developer_message = "Migration error: {message}",
        status = 500
    )]
    Migration {
        /// Error message.
        message: String,
        /// The migration version (if applicable).
        version: Option<i64>,
    },

    /// The commit was sent and no answer came back, so whether it applied is unknown.
    ///
    /// Never retryable: the server may have applied it, so running the body again could apply it twice.
    #[error(
        code = "DB_012",
        message = "We could not confirm whether the change was saved. Check it before trying again.",
        developer_message = "commit outcome unknown: {message}",
        status = 500
    )]
    CommitOutcomeUnknown {
        /// What the connection reported when the commit's answer was lost.
        message: String,
    },

    /// Internal/unexpected error.
    #[error(
        code = "DB_011",
        message = "Something went wrong while trying to execute a database query.",
        developer_message = "Internal database error: {message}",
        status = 500
    )]
    Internal {
        /// Error message.
        message: String,
    },
}

impl DatabaseError {
    /// Creates a connection error.
    pub fn connection(message: impl Into<String>, retryable: bool) -> Self {
        Self::Connection {
            message: message.into(),
            retryable,
        }
    }

    /// Creates a query error.
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            query: None,
        }
    }

    /// Creates a query error with the failing query.
    pub fn query_with_sql(message: impl Into<String>, query: impl Into<String>) -> Self {
        Self::Query {
            message: message.into(),
            query: Some(query.into()),
        }
    }

    /// Creates a unique constraint violation error.
    pub fn unique_violation(constraint: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            constraint: constraint.into(),
            kind: ConstraintKind::Unique,
        }
    }

    /// Creates a foreign key violation error.
    pub fn foreign_key_violation(constraint: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            constraint: constraint.into(),
            kind: ConstraintKind::ForeignKey,
        }
    }

    /// Creates a not null violation error.
    pub fn not_null_violation(column: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            constraint: column.into(),
            kind: ConstraintKind::NotNull,
        }
    }

    /// Creates a check constraint violation error.
    pub fn check_violation(constraint: impl Into<String>) -> Self {
        Self::ConstraintViolation {
            constraint: constraint.into(),
            kind: ConstraintKind::Check,
        }
    }

    /// Creates a not found error.
    pub fn not_found(entity: impl Into<String>, id: impl fmt::Display) -> Self {
        Self::NotFound {
            entity: entity.into(),
            id: id.to_string(),
        }
    }

    /// Creates a transaction error.
    pub fn transaction(message: impl Into<String>, rolled_back: bool) -> Self {
        Self::Transaction {
            message: message.into(),
            rolled_back,
        }
    }

    /// Creates a serialization error.
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Creates a timeout error.
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::Timeout { timeout_ms }
    }

    /// Creates a pool-exhaustion error for `pool`, which refused after `waited_ms`.
    pub fn pool_exhausted(pool: impl Into<String>, waited_ms: u64) -> Self {
        Self::PoolExhausted {
            pool: pool.into(),
            waited_ms,
        }
    }

    /// Creates the error for a commit whose answer never arrived.
    pub fn commit_outcome_unknown(message: impl Into<String>) -> Self {
        Self::CommitOutcomeUnknown {
            message: message.into(),
        }
    }

    /// Creates a configuration error.
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Creates a migration error.
    pub fn migration(message: impl Into<String>) -> Self {
        Self::Migration {
            message: message.into(),
            version: None,
        }
    }

    /// Creates a migration error with version.
    pub fn migration_version(message: impl Into<String>, version: i64) -> Self {
        Self::Migration {
            message: message.into(),
            version: Some(version),
        }
    }

    /// Creates an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Returns whether this error is retryable.
    ///
    /// [`Other`](Self::Other) delegates to the error it wraps, where an adapter's conflict lands.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection { retryable, .. } => *retryable,
            Self::PoolExhausted { .. } => true,
            Self::Timeout { .. } => true,
            Self::Transaction { rolled_back, .. } => *rolled_back,
            Self::Other(inner) => inner.get().is_retryable(),
            _ => false,
        }
    }

    /// Returns whether this is a constraint violation.
    pub fn is_constraint_violation(&self) -> bool {
        matches!(self, Self::ConstraintViolation { .. })
    }

    /// Returns whether this is a not found error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    /// Returns the constraint kind if this is a constraint violation.
    pub fn constraint_kind(&self) -> Option<ConstraintKind> {
        match self {
            Self::ConstraintViolation { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error;

    #[test]
    fn a_commit_whose_outcome_is_unknown_is_never_retryable() {
        let error = DatabaseError::commit_outcome_unknown("connection reset");
        assert!(!error.is_retryable());
        assert_eq!(error.error_code(), "DB_012");
        assert_eq!(error.http_status_code(), 500);
    }

    #[test]
    fn test_error_retryable() {
        assert!(DatabaseError::connection("failed", true).is_retryable());
        assert!(!DatabaseError::connection("failed", false).is_retryable());
        assert!(DatabaseError::timeout(1000).is_retryable());
        assert!(!DatabaseError::not_found("User", "123").is_retryable());
    }

    /// A wrapped error keeps its own answer, because the retry loops read this one.
    #[test]
    fn a_wrapped_retryable_error_stays_retryable() {
        #[derive(Debug, fundamental_macros::Error)]
        enum AdapterError {
            #[error(
                code = "PROBE_001",
                message = "Serialization conflict",
                status = 409,
                retryable = true
            )]
            Conflict,
            #[error(code = "PROBE_002", message = "Bad request", status = 400)]
            Fatal,
        }

        let retryable = DatabaseError::Other(AnyError::new(AdapterError::Conflict));
        let fatal = DatabaseError::Other(AnyError::new(AdapterError::Fatal));

        assert!(
            retryable.is_retryable(),
            "a wrapped conflict must still ask to be retried"
        );
        assert!(!fatal.is_retryable(), "and a wrapped fatal must not");

        assert_eq!(
            DatabaseError::is_retryable(&retryable),
            Error::is_retryable(&retryable)
        );
        assert_eq!(
            DatabaseError::is_retryable(&fatal),
            Error::is_retryable(&fatal)
        );
    }

    #[test]
    fn test_constraint_kind() {
        let err = DatabaseError::unique_violation("users_email_key");
        assert_eq!(err.constraint_kind(), Some(ConstraintKind::Unique));
    }

    #[test]
    fn test_http_status_codes() {
        assert_eq!(
            DatabaseError::not_found("User", "123").http_status_code(),
            404
        );
        assert_eq!(
            DatabaseError::unique_violation("key").http_status_code(),
            409
        );
        assert_eq!(DatabaseError::timeout(1000).http_status_code(), 503);
    }
}
