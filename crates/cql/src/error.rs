//! YCQL error types and retry classification.

use fundamental_core::AnyError;
use fundamental_db::DatabaseError;
use fundamental_db::migration::{GuardError, LedgerError};
use fundamental_macros::Error;

use scylla::errors::{DbError, ExecutionError, NewSessionError, RequestAttemptError};

/// YCQL-specific error type.
#[derive(Debug, Error)]
#[error_into(DatabaseError)]
#[non_exhaustive]
pub enum CqlError {
    /// A wrapped error that does not map onto a more specific variant,
    /// preserving the original [`AnyError`] cause.
    Other(AnyError),
    /// Query execution error from the driver.
    ///
    /// Boxed: the driver's [`ExecutionError`] is large.
    #[error(code = "CQL_001", message = "Query error: {0}", status = 500)]
    Query(#[source] Box<ExecutionError>),

    /// Session creation error.
    ///
    /// Boxed: the driver's [`NewSessionError`] is large.
    #[error(code = "CQL_002", message = "Session error: {0}", status = 500)]
    Session(#[source] Box<NewSessionError>),

    /// Row conversion error.
    #[error(code = "CQL_003", message = "Row conversion error: {0}", status = 500)]
    RowConversion(String),

    /// Prepared statement error.
    #[error(
        code = "CQL_004",
        message = "Prepared statement error: {0}",
        status = 500
    )]
    Prepared(String),

    /// Configuration error.
    #[error(code = "CQL_005", message = "Configuration error: {0}", status = 500)]
    Config(String),

    /// Timeout error with optional duration information.
    #[error(
        code = "CQL_006",
        message = "Operation timed out after {timeout_ms}ms: {message}",
        status = 500
    )]
    Timeout {
        /// The timeout error message.
        message: String,
        /// The timeout duration in milliseconds (if known).
        timeout_ms: Option<u64>,
    },

    /// Unavailable error (not enough replicas).
    #[error(
        code = "CQL_007",
        message = "Not enough replicas available: required {required}, alive {alive}",
        status = 500
    )]
    Unavailable {
        /// Required number of replicas.
        required: i32,
        /// Number of alive replicas.
        alive: i32,
    },

    /// Write timeout.
    #[error(
        code = "CQL_008",
        message = "Write timeout: received {received} of {required} acknowledgments",
        status = 500
    )]
    WriteTimeout {
        /// Required acknowledgments.
        required: i32,
        /// Received acknowledgments.
        received: i32,
    },

    /// Read timeout.
    #[error(
        code = "CQL_009",
        message = "Read timeout: received {received} of {required} responses",
        status = 500
    )]
    ReadTimeout {
        /// Required responses.
        required: i32,
        /// Received responses.
        received: i32,
    },

    /// Rate limiting error.
    #[error(code = "CQL_010", message = "Rate limited: {0}", status = 500)]
    RateLimited(String),

    /// Invalid query (missing partition key, etc.).
    #[error(code = "CQL_011", message = "Invalid query: {0}", status = 500)]
    InvalidQuery(String),

    /// A caller-supplied identifier (column name) failed the safe-identifier
    /// rule and was refused before reaching CQL text.
    ///
    /// A client error (`400`), never retried.
    #[error(code = "CQL_012", message = "Invalid identifier: {0}", status = 400)]
    InvalidIdentifier(String),

    /// The live schema could not be read back into a comparable form.
    #[error(
        code = "CQL_013",
        message = "Schema introspection failed: {0}",
        status = 500
    )]
    Introspection(String),

    /// A migration statement failed to apply.
    ///
    /// CQL has no multi-statement transaction, so the statements before `step` stayed applied.
    #[error(
        code = "CQL_014",
        message = "Migration '{name}' failed at step {step} of {total}: {cause}",
        status = 500
    )]
    MigrationFailed {
        /// Migration name.
        name: String,
        /// Which step failed, counting from one: each statement is a step, as the step ledger
        /// records it.
        step: usize,
        /// How many steps the migration has.
        total: usize,
        /// The statement the step ran.
        statement: String,
        /// What the driver reported, kept whole.
        #[source]
        #[overrides]
        cause: crate::CqlMigrationCause,
    },

    /// Every statement of a migration applied, and the ledger row recording it did not.
    ///
    /// The opposite of [`MigrationFailed`](Self::MigrationFailed): the next run applies it a second
    /// time, and nothing undoes the first.
    #[error(
        code = "CQL_015",
        message = "Migration '{name}' applied but was not recorded, so the next run will \
                   apply it again: {cause}",
        status = 500
    )]
    MigrationRecordFailed {
        /// Migration name.
        name: String,
        /// What the driver reported, kept whole.
        #[source]
        #[overrides]
        cause: crate::CqlMigrationCause,
    },

    /// An applied migration's content no longer matches what was applied.
    ///
    /// Refused rather than repaired: only the edit's author knows what it meant.
    #[error(
        code = "CQL_016",
        message = "Migration '{name}' has changed since it was applied \
                   (applied {applied}, found {found}). The keyspace and the \
                   migration files have diverged.",
        status = 500
    )]
    MigrationChanged {
        /// Migration name.
        name: String,
        /// The content hash recorded when it was applied.
        applied: String,
        /// The content hash of the file on disk now.
        found: String,
    },

    /// A paged read failed for a reason this enum has no more specific name for.
    ///
    /// The driver's errors are `#[non_exhaustive]`; named transport failures widen onto
    /// [`CqlError::Query`] instead.
    #[error(code = "CQL_017", message = "Paged read failed: {0}", status = 500)]
    Paged(String),

    /// A conditional write ran, and its result did not say whether it applied.
    ///
    /// Not `Ok(false)`: a single-use ledger reading this as "spent" would refuse a first use.
    /// Retryable.
    #[error(
        code = "CQL_018",
        message = "The store could not confirm whether a conditional write applied ({reason})",
        status = 503,
        retryable
    )]
    LwtUnconfirmed {
        /// Which part of the result could not be read, from a closed set of codes.
        reason: &'static str,
    },

    /// Another migrator held this keyspace's migration lock for longer than this one waits.
    ///
    /// Retryable: the holder will finish, or its lease expires.
    #[error(
        code = "CQL_019",
        message = "Another migrator held the migration lock on {keyspace} for {waited_secs}s",
        status = 503,
        retryable
    )]
    MigrationLockHeld {
        /// The keyspace whose migrations are locked.
        keyspace: String,
        /// How long this migrator waited.
        waited_secs: u64,
    },

    /// This migrator's lease on the migration lock expired or was taken before its next
    /// statement, so it stopped rather than run beside another.
    #[error(
        code = "CQL_020",
        message = "The migration lock on {keyspace} was lost before migration {migration} finished",
        status = 503,
        retryable
    )]
    MigrationLockLost {
        /// The keyspace whose migrations are locked.
        keyspace: String,
        /// The migration that was running.
        migration: String,
    },

    /// The migration guard refused the set before any of it ran.
    ///
    /// Transparent, so the line the guard names reaches the reader.
    #[error(transparent)]
    Refused(#[from] GuardError),

    /// The migration ledger refused: an apply past a failed migration that nothing has
    /// settled, or a resolution that cannot be proved or recorded.
    #[error(transparent)]
    Ledger(#[from] LedgerError),
}

impl CqlError {
    /// Returns whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Query(e) => is_execution_error_retryable(e),
            Self::Timeout { .. } => true,
            Self::Unavailable { .. } => true,
            Self::WriteTimeout { .. } => true,
            Self::ReadTimeout { .. } => true,
            Self::RateLimited(_) => true,
            Self::LwtUnconfirmed { .. } => true,
            Self::MigrationLockHeld { .. } => true,
            Self::MigrationLockLost { .. } => true,
            // The two arms that also answer through `dyn Error`, because the field each reads
            // is `#[overrides]` and the derive consults an override before its own literal.
            Self::MigrationFailed { cause, .. } => cause.is_retryable(),
            Self::MigrationRecordFailed { cause, .. } => cause.is_retryable(),
            _ => false,
        }
    }

    /// Creates a timeout error with a message.
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout {
            message: msg.into(),
            timeout_ms: None,
        }
    }

    /// Creates a timeout error with a message and duration.
    pub fn timeout_with_duration(msg: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout {
            message: msg.into(),
            timeout_ms: Some(timeout_ms),
        }
    }

    /// Creates a row conversion error.
    pub fn row_conversion(msg: impl Into<String>) -> Self {
        Self::RowConversion(msg.into())
    }

    /// Creates a prepared statement error.
    pub fn prepared(msg: impl Into<String>) -> Self {
        Self::Prepared(msg.into())
    }

    /// Creates a configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Creates an invalid query error.
    pub fn invalid_query(msg: impl Into<String>) -> Self {
        Self::InvalidQuery(msg.into())
    }

    /// Creates an invalid-identifier error (rejected column name).
    pub fn invalid_identifier(msg: impl Into<String>) -> Self {
        Self::InvalidIdentifier(msg.into())
    }

    /// Creates a paged-read error for a driver failure with no specific variant.
    pub fn paged(msg: impl Into<String>) -> Self {
        Self::Paged(msg.into())
    }
}

/// Checks if an ExecutionError is retryable.
fn is_execution_error_retryable(err: &ExecutionError) -> bool {
    match err {
        ExecutionError::RequestTimeout(_) => true,
        ExecutionError::ConnectionPoolError(_) => true,
        ExecutionError::LastAttemptError(attempt_err) => {
            is_request_attempt_error_retryable(attempt_err)
        }
        _ => false,
    }
}

/// Checks if a RequestAttemptError is retryable.
fn is_request_attempt_error_retryable(err: &RequestAttemptError) -> bool {
    match err {
        RequestAttemptError::DbError(db_err, _) => matches!(
            db_err,
            DbError::Unavailable { .. }
                | DbError::ReadTimeout { .. }
                | DbError::WriteTimeout { .. }
                | DbError::Overloaded
                | DbError::IsBootstrapping
                | DbError::RateLimitReached { .. }
        ),
        RequestAttemptError::BrokenConnectionError(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_errors() {
        let timeout = CqlError::timeout("test");
        assert!(timeout.is_retryable());

        let unavailable = CqlError::Unavailable {
            required: 3,
            alive: 1,
        };
        assert!(unavailable.is_retryable());

        let config = CqlError::Config("bad config".into());
        assert!(!config.is_retryable());

        // A rejected identifier is a client error, never retried.
        let bad_ident = CqlError::invalid_identifier("col; DROP");
        assert!(!bad_ident.is_retryable());
    }

    #[test]
    fn an_unconfirmed_conditional_write_is_retryable_on_both_paths() {
        use fundamental_core::Error as _;

        let unconfirmed = CqlError::LwtUnconfirmed { reason: "no_rows" };
        assert_eq!(unconfirmed.error_code(), "CQL_018");
        assert_eq!(unconfirmed.http_status_code(), 503);
        assert!(unconfirmed.is_retryable());
        // The route a repository takes: through `DatabaseError::Other`, which asks the error
        // it wraps. A string conversion here would read as a permanent failure.
        let database: DatabaseError = unconfirmed.into();
        assert!(database.is_retryable());
    }

    #[test]
    fn test_invalid_identifier_code_and_status() {
        use fundamental_core::Error;

        let err = CqlError::invalid_identifier("x; DROP TABLE t; --");
        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
        assert_eq!(err.error_code(), "CQL_012");
        assert_eq!(err.http_status_code(), 400);
    }

    #[test]
    fn test_error_constructors() {
        let err = CqlError::row_conversion("bad row");
        assert!(matches!(err, CqlError::RowConversion(_)));

        let err = CqlError::invalid_query("missing partition key");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn test_timeout_with_duration() {
        let err = CqlError::timeout_with_duration("query timed out", 5000);
        match err {
            CqlError::Timeout {
                message,
                timeout_ms,
            } => {
                assert_eq!(message, "query timed out");
                assert_eq!(timeout_ms, Some(5000));
            }
            _ => panic!("Expected Timeout variant"),
        }
    }

    #[test]
    fn test_timeout_display() {
        let err_with = CqlError::timeout_with_duration("query", 1000);
        assert!(err_with.to_string().contains("1000ms"));

        let err_without = CqlError::timeout("query");
        assert!(err_without.to_string().contains("None(timeout_ms)"));
    }
}
