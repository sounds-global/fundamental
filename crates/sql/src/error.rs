//! Error type for the YugabyteDB YSQL adapter.

use fundamental_core::AnyError;
use fundamental_core::outbox::RefusalKind;
use fundamental_db::migration::{GuardError, LedgerError};
use fundamental_db::{ConstraintKind, DatabaseError};
use fundamental_macros::Error;

/// The code [`DatabaseError::PoolExhausted`] carries; a test keeps the two in step.
const POOL_EXHAUSTED: &str = "DB_007";

/// Whether `error` is the pool refusing an acquire, judged by its stable code so it survives the caller's `E`.
pub(crate) fn is_pool_exhaustion<E: fundamental_core::Error + ?Sized>(error: &E) -> bool {
    error.error_code() == POOL_EXHAUSTED
}

/// `internal_error`, PostgreSQL's catch-all; never retryable as a class.
const EXPIRED: &str = "XX000";

/// The phrase both of YugabyteDB's wordings for an untagged expired-transaction abort carry.
const EXPIRED_MARKER: &str = "expired transaction";

/// Errors produced by the YugabyteDB YSQL adapter.
#[derive(Debug, Error)]
#[error_into(DatabaseError)]
#[non_exhaustive]
pub enum SqlError {
    /// Catch-all for errors that do not fit a more specific variant.
    Other(AnyError),
    /// SQLx driver error not classified into a typed variant; a `500` whose wire message names nothing about the store.
    #[error(
        code = "SQL_001",
        message = "The database could not complete the request",
        developer_message = "Database error: {0}",
        status = 500
    )]
    Sqlx(#[error(skip_param)] sqlx::Error),

    /// Connection pool error.
    #[error(code = "SQL_002", message = "Pool error: {0}", status = 500)]
    Pool(String),

    /// Serialization conflict (SQLSTATE 40001).
    #[error(
        code = "SQL_003",
        message = "Serialization conflict: {message}",
        status = 409,
        retryable = true
    )]
    SerializationConflict {
        /// Error message.
        message: String,
    },

    /// Deadlock detected (SQLSTATE 40P01).
    #[error(
        code = "SQL_004",
        message = "Deadlock detected: {message}",
        status = 409,
        retryable = true
    )]
    Deadlock {
        /// Error message.
        message: String,
    },

    /// The engine aborted the transaction for missing its heartbeats, reported untagged as `XX000`; safe to run again.
    #[error(
        code = "SQL_015",
        message = "Transaction expired: {message}",
        status = 409,
        retryable = true
    )]
    TransactionExpired {
        /// What the engine reported.
        message: String,
    },

    /// A stored row could not be decoded into its model, for a reason the row or this process's keys decide.
    #[error(
        code = "SQL_016",
        message = "A row of {table} refused to decode at column {column}: {kind}",
        status = 500
    )]
    RowRefused {
        /// The table the row was read from.
        table: &'static str,
        /// The first column that refused.
        column: &'static str,
        /// Why it refused.
        kind: RefusalKind,
    },

    /// An outbox table's key column has a type whose text copy does not cast back exactly.
    #[error(
        code = "SQL_017",
        message = "An outbox table's key cannot address a refused row",
        developer_message = "key column {column} of {table} has type {type_name}, which a claim cannot address a row by",
        status = 500
    )]
    UnaddressableKey {
        /// The outbox table.
        table: &'static str,
        /// The key column.
        column: &'static str,
        /// The column's database type.
        type_name: String,
    },

    /// A claim's write to a row it holds the lock on did not land on exactly one row.
    #[error(
        code = "SQL_018",
        message = "An outbox claim could not mark a row it holds",
        developer_message = "a claim's set-aside or skip on {table} affected {affected} rows, not one",
        status = 500
    )]
    OutboxMarkMissed {
        /// The outbox table.
        table: &'static str,
        /// How many rows the write affected.
        affected: u64,
    },

    /// The transaction's body returned success after one of its statements failed, so it was rolled back.
    #[error(
        code = "SQL_019",
        message = "The transaction was not committed because one of its statements failed",
        developer_message = "a statement failed with SQLSTATE {sqlstate} and the body still returned Ok; the transaction was rolled back",
        status = 500
    )]
    TransactionAborted {
        /// The SQLSTATE of the statement that aborted the transaction.
        sqlstate: String,
    },

    /// Constraint violation with context.
    #[error(
        code = "SQL_005",
        message = "Constraint violation on {table} {constraint}: {message}",
        status = 400
    )]
    ConstraintViolation {
        /// Error message.
        message: String,
        /// The table involved (if known).
        table: Option<String>,
        /// The constraint name (if known).
        constraint: Option<String>,
        /// The kind of constraint.
        kind: ConstraintKind,
    },

    /// Query building error.
    #[error(code = "SQL_006", message = "Query error: {0}", status = 400)]
    Query(String),

    /// Row conversion error.
    #[error(code = "SQL_007", message = "Row conversion error: {0}", status = 400)]
    RowConversion(String),

    /// Configuration error.
    #[error(code = "SQL_008", message = "Configuration error: {0}", status = 500)]
    Config(String),

    /// The live schema could not be read back into a comparable form.
    #[error(
        code = "SQL_010",
        message = "Schema introspection failed: {0}",
        status = 500
    )]
    Introspection(String),

    /// A migration failed to apply, carrying the engine's failure and which step failed.
    #[error(
        code = "SQL_011",
        message = "Migration '{name}' failed at step {step} of {total}: {cause}",
        status = 500
    )]
    MigrationFailed {
        /// Migration name.
        name: String,
        /// Which step failed, counting from one.
        step: usize,
        /// How many steps the migration has.
        total: usize,
        /// The statement the step ran.
        statement: String,
        /// What the engine reported, kept whole.
        #[source]
        #[overrides]
        cause: crate::MigrationCause,
    },

    /// An applied migration's content no longer matches what was applied.
    #[error(
        code = "SQL_013",
        message = "Migration '{name}' has changed since it was applied \
                   (applied {applied}, found {found}). The database and the \
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

    /// A namespace was addressed and the catalog does not have it.
    #[error(
        code = "SQL_014",
        message = "Schema '{schema}' is not in the catalog",
        status = 500
    )]
    SchemaMissing {
        /// The namespace that was addressed.
        schema: String,
    },

    /// The migration guard refused the set before any of it ran.
    #[error(transparent)]
    Refused(#[from] GuardError),

    /// The migration ledger refused an apply or a resolution.
    #[error(transparent)]
    Ledger(#[from] LedgerError),
}

impl From<sqlx::Error> for SqlError {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn from(err: sqlx::Error) -> Self {
        use sqlx::error::ErrorKind;

        if let sqlx::Error::Database(db) = &err {
            if let Some(code) = db.code() {
                match code.as_ref() {
                    "40001" => {
                        return Self::SerializationConflict {
                            message: db.message().to_string(),
                        };
                    }
                    "40P01" => {
                        return Self::Deadlock {
                            message: db.message().to_string(),
                        };
                    }
                    _ => {}
                }
            }

            if is_expired_transaction_error(&err) {
                let message = db.message().to_string();
                return Self::TransactionExpired { message };
            }

            let kind = match db.kind() {
                ErrorKind::UniqueViolation => Some(ConstraintKind::Unique),
                ErrorKind::ForeignKeyViolation => Some(ConstraintKind::ForeignKey),
                ErrorKind::NotNullViolation => Some(ConstraintKind::NotNull),
                ErrorKind::CheckViolation => Some(ConstraintKind::Check),
                _ => None,
            };

            if let Some(kind) = kind {
                return Self::ConstraintViolation {
                    message: db.message().to_string(),
                    table: db.table().map(|t| t.to_string()),
                    // YugabyteDB names the constraint only in its message, leaving the protocol field empty.
                    constraint: db
                        .constraint()
                        .map(|c| c.to_string())
                        .or_else(|| constraint_named_in(db.message())),
                    kind,
                };
            }
        }

        Self::Sqlx(err)
    }
}

/// The constraint a PostgreSQL-wire violation message names, or `None` when the message is not that shape.
fn constraint_named_in(message: &str) -> Option<String> {
    let after = message.split_once(" constraint \"")?.1;
    let name = after.split_once('"')?.0;
    (!name.is_empty()).then(|| name.to_owned())
}

impl SqlError {
    /// The SQLSTATE of an error the server raised for a statement, which aborts its transaction; `None` for a local error.
    #[must_use]
    pub fn aborting_sqlstate(&self) -> Option<String> {
        if let Some(code) = self.sqlstate() {
            return Some(code.to_owned());
        }
        match self {
            Self::Sqlx(sqlx::Error::Database(db)) => Some(
                db.code()
                    .map_or_else(|| "unknown".to_owned(), |code| code.into_owned()),
            ),
            Self::ConstraintViolation { kind, .. } => Some(
                match kind {
                    ConstraintKind::Unique => "23505",
                    ConstraintKind::ForeignKey => "23503",
                    ConstraintKind::NotNull => "23502",
                    ConstraintKind::Check => "23514",
                }
                .to_owned(),
            ),
            Self::MigrationFailed { cause, .. } => cause.error().aborting_sqlstate(),
            _ => None,
        }
    }

    /// Whether the server's answer was lost rather than given: a dropped connection, or SQLSTATE class `08` or `57`.
    #[must_use]
    pub fn loses_the_answer(&self) -> bool {
        match self {
            Self::Sqlx(
                sqlx::Error::Io(_)
                | sqlx::Error::Protocol(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::WorkerCrashed,
            ) => true,
            Self::Sqlx(sqlx::Error::Database(db)) => db
                .code()
                .is_some_and(|code| code.starts_with("08") || code.starts_with("57")),
            _ => false,
        }
    }

    /// The SQLSTATE class this error came from, where it came from one.
    #[must_use]
    pub fn sqlstate(&self) -> Option<&'static str> {
        match self {
            Self::SerializationConflict { .. } => Some("40001"),
            Self::Deadlock { .. } => Some("40P01"),
            Self::Sqlx(e) if is_serialization_error(e) => Some("40001"),
            Self::Sqlx(e) if is_deadlock_error(e) => Some("40P01"),
            // Reported as `XX000`, as the engine said, so the retry metric does not lie.
            Self::TransactionExpired { .. } => Some(EXPIRED),
            Self::Sqlx(e) if is_expired_transaction_error(e) => Some(EXPIRED),
            Self::MigrationFailed { cause, .. } => cause.sqlstate(),
            _ => None,
        }
    }

    /// Returns whether this error is a serialization conflict.
    pub fn is_serialization_conflict(&self) -> bool {
        match self {
            Self::SerializationConflict { .. } => true,
            Self::Sqlx(e) => is_serialization_error(e),
            Self::MigrationFailed { cause, .. } => cause.error().is_serialization_conflict(),
            _ => false,
        }
    }

    /// Returns whether this error is a deadlock.
    pub fn is_deadlock(&self) -> bool {
        match self {
            Self::Deadlock { .. } => true,
            Self::Sqlx(e) => is_deadlock_error(e),
            Self::MigrationFailed { cause, .. } => cause.error().is_deadlock(),
            _ => false,
        }
    }

    /// Returns whether the engine reported `42P01 undefined_table`; deliberately not [`is_retryable`](Self::is_retryable).
    pub fn is_undefined_table(&self) -> bool {
        match self {
            Self::Sqlx(e) => is_undefined_table_error(e),
            _ => false,
        }
    }

    /// Returns whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::SerializationConflict { .. } => true,
            Self::Deadlock { .. } => true,
            Self::TransactionExpired { .. } => true,
            Self::Sqlx(e) => is_retryable_error(e),
            // The field is `#[overrides]`, so this also answers through `dyn Error`.
            Self::MigrationFailed { cause, .. } => cause.is_retryable(),
            // A classified error arrives here after crossing `AnyError`; answer as it does.
            Self::Other(inner) => inner.get().is_retryable(),
            _ => false,
        }
    }

    /// Whether this crate should run the operation again itself: as [`is_retryable`](Self::is_retryable),
    /// except that a saturated pool is not asked again.
    #[must_use]
    pub fn should_retry_in_process(&self) -> bool {
        if self.is_pool_exhaustion() {
            return false;
        }
        self.is_retryable()
    }

    /// Returns whether this is the pool refusing an acquire within its declared bound.
    #[must_use]
    pub fn is_pool_exhaustion(&self) -> bool {
        is_pool_exhaustion(self)
    }

    /// Creates a serialization conflict error.
    pub fn serialization_conflict(message: impl Into<String>) -> Self {
        Self::SerializationConflict {
            message: message.into(),
        }
    }

    /// Creates a deadlock error.
    pub fn deadlock(message: impl Into<String>) -> Self {
        Self::Deadlock {
            message: message.into(),
        }
    }

    /// Creates a constraint violation error.
    pub fn constraint_violation(message: impl Into<String>, kind: ConstraintKind) -> Self {
        Self::ConstraintViolation {
            message: message.into(),
            table: None,
            constraint: None,
            kind,
        }
    }

    /// Creates a constraint violation error with table context.
    pub fn constraint_violation_with_context(
        message: impl Into<String>,
        table: impl Into<String>,
        constraint: Option<String>,
        kind: ConstraintKind,
    ) -> Self {
        Self::ConstraintViolation {
            message: message.into(),
            table: Some(table.into()),
            constraint,
            kind,
        }
    }

    /// Creates a query error.
    pub fn query(message: impl Into<String>) -> Self {
        Self::Query(message.into())
    }

    /// Creates a row conversion error.
    pub fn row_conversion(message: impl Into<String>) -> Self {
        Self::RowConversion(message.into())
    }

    /// Adds table context to a constraint violation error; any other error is returned unchanged.
    pub fn with_table(self, table: impl Into<String>) -> Self {
        match self {
            Self::ConstraintViolation {
                message,
                constraint,
                kind,
                ..
            } => Self::ConstraintViolation {
                message,
                table: Some(table.into()),
                constraint,
                kind,
            },
            other => other,
        }
    }
}

fn is_serialization_error(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        // SQLSTATE serialization_failure
        if let Some(code) = db_err.code() {
            return code == "40001";
        }
    }
    false
}

fn is_undefined_table_error(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        // SQLSTATE undefined_table
        if let Some(code) = db_err.code() {
            return code == "42P01";
        }
    }
    false
}

fn is_deadlock_error(err: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = err {
        // SQLSTATE deadlock_detected
        if let Some(code) = db_err.code() {
            return code == "40P01";
        }
    }
    false
}

/// Whether `message` is the engine's wording for a transaction it expired, ASCII-case-insensitively.
///
/// Unanchored is safe only because identifier validation admits no whitespace; widening it would break this.
fn is_expired_transaction_message(message: &str) -> bool {
    message.to_ascii_lowercase().contains(EXPIRED_MARKER)
}

/// Checks if an SQLx error is the engine's untagged transaction abort.
fn is_expired_transaction_error(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            db_err.code().as_deref() == Some(EXPIRED)
                && is_expired_transaction_message(db_err.message())
        }
        _ => false,
    }
}

fn is_retryable_error(err: &sqlx::Error) -> bool {
    if is_expired_transaction_error(err) {
        return true;
    }
    match err {
        sqlx::Error::Io(_) => true,
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::PoolClosed => false,
        sqlx::Error::Database(db_err) => {
            if let Some(code) = db_err.code() {
                matches!(
                    code.as_ref(),
                    "40001" | "40P01" | "57P01" | "57P02" | "57P03"
                )
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_constraint_is_named_from_the_engines_own_message() {
        for (message, name) in [
            (
                "duplicate key value violates unique constraint \"tenant_entities_by_skeleton\"",
                Some("tenant_entities_by_skeleton"),
            ),
            (
                "new row for relation \"evidence_records\" violates check constraint \"evidence_records_check\"",
                Some("evidence_records_check"),
            ),
            (
                "insert or update on table \"entity_roles\" violates foreign key constraint \"entity_roles_entity_id_fkey\"",
                Some("entity_roles_entity_id_fkey"),
            ),
            ("duplicate key value violates unique constraint", None),
            ("some other failure", None),
        ] {
            assert_eq!(
                super::constraint_named_in(message).as_deref(),
                name,
                "{message}"
            );
        }
    }

    use super::*;
    use fundamental_core::Error;

    #[test]
    fn test_serialization_conflict() {
        let err = SqlError::serialization_conflict("test conflict");
        assert!(err.is_serialization_conflict());
        assert!(err.is_retryable());
    }

    #[test]
    fn an_error_that_is_not_a_database_error_is_not_an_undefined_table() {
        let err = SqlError::serialization_conflict("relation does not exist");
        assert!(!err.is_undefined_table());
        assert!(!SqlError::serialization_conflict("x").is_undefined_table());
    }

    #[test]
    fn test_deadlock() {
        let err = SqlError::deadlock("test deadlock");
        assert!(err.is_deadlock());
        assert!(err.is_retryable());
        assert_eq!(err.error_code(), "SQL_004");
    }

    #[test]
    fn test_constraint_violation() {
        let err = SqlError::constraint_violation_with_context(
            "duplicate key",
            "users",
            Some("users_email_key".to_string()),
            ConstraintKind::Unique,
        );

        match err {
            SqlError::ConstraintViolation {
                table,
                constraint,
                kind,
                ..
            } => {
                assert_eq!(table, Some("users".to_string()));
                assert_eq!(constraint, Some("users_email_key".to_string()));
                assert_eq!(kind, ConstraintKind::Unique);
            }
            _ => panic!("Expected ConstraintViolation"),
        }
    }

    #[test]
    fn test_with_table() {
        let err =
            SqlError::constraint_violation("test", ConstraintKind::Unique).with_table("users");

        match err {
            SqlError::ConstraintViolation { table, .. } => {
                assert_eq!(table, Some("users".to_string()));
            }
            _ => panic!("Expected ConstraintViolation"),
        }
    }

    #[test]
    fn test_non_retryable_error() {
        let err = SqlError::Config("bad config".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn the_pool_exhaustion_code_is_the_variants_own() {
        assert_eq!(
            DatabaseError::pool_exhausted("sql.layer0", 2_000).error_code(),
            POOL_EXHAUSTED,
            "the constant and the variant disagree, so the refusal is no longer recognised"
        );
    }

    #[test]
    fn a_row_refusal_is_not_retryable_and_names_no_value() {
        let refused = SqlError::RowRefused {
            table: "resolution_events",
            column: "grant_id",
            kind: RefusalKind::DecryptRefused,
        };

        assert_eq!(refused.error_code(), "SQL_016");
        assert_eq!(refused.http_status_code(), 500);
        assert!(
            !refused.is_retryable(),
            "a row that refuses to decode refuses again; retrying it is a hot loop"
        );
        assert_eq!(
            refused.to_string(),
            "A row of resolution_events refused to decode at column grant_id: decrypt_refused"
        );
    }

    #[test]
    fn an_unclassified_driver_error_tells_the_caller_nothing_about_the_store() {
        let error = SqlError::Sqlx(sqlx::Error::Protocol(
            "column \"commitment_scheme\" of relation \"audit_outboxes\" does not exist"
                .to_string(),
        ));
        assert_eq!(error.error_code(), "SQL_001");
        assert_eq!(error.http_status_code(), 500);
        let wire = error.display_message();
        assert!(
            !wire.contains("commitment_scheme") && !wire.contains("audit_outboxes"),
            "the wire message names the schema: {wire}"
        );
        assert!(
            error
                .developer_message()
                .is_some_and(|line| line.contains("audit_outboxes")),
            "the operator is owed the driver's sentence"
        );
    }

    #[test]
    fn a_pool_refusal_tells_the_caller_nothing_about_the_store() {
        let refusal = DatabaseError::pool_exhausted("sql.layer0", 2_000);

        assert_eq!(refusal.error_code(), "DB_007");
        assert_eq!(refusal.http_status_code(), 503);
        assert!(
            refusal.is_retryable(),
            "a full pool drains; the caller may return"
        );

        let wire = refusal.display_message();
        assert!(
            !wire.contains("sql.layer0") && !wire.contains("2000"),
            "the wire message leaks the pool or the wait: {wire}"
        );
        assert!(
            refusal
                .developer_message()
                .is_some_and(|line| line.contains("sql.layer0") && line.contains("2000")),
            "the operator is owed both facts the caller is not"
        );
    }

    #[test]
    fn a_pool_refusal_is_retryable_for_the_caller_and_never_for_the_loop() {
        let refused = SqlError::Other(AnyError::new(DatabaseError::pool_exhausted("p", 1)));

        assert!(refused.is_pool_exhaustion());
        assert!(
            refused.is_retryable(),
            "the caller may try again once the pool has drained"
        );
        assert!(
            !refused.should_retry_in_process(),
            "retrying an acquire against an exhausted pool raises the pressure that caused it"
        );
    }

    #[test]
    fn a_serialization_conflict_still_retries_in_process() {
        let conflict = SqlError::serialization_conflict("40001");

        assert!(!conflict.is_pool_exhaustion());
        assert!(conflict.should_retry_in_process());
    }
}
