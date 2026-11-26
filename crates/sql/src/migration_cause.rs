use std::error::Error as StdError;
use std::fmt;

use fundamental_core::ErrorOverride;

use crate::SqlError;

/// The failure a migration ended on, carried whole so its verdict outlives the runner.
///
/// Held in an `#[overrides]` field, so `is_retryable` answers truthfully through `dyn Error` too.
#[derive(Debug)]
pub struct MigrationCause(Box<SqlError>);

impl MigrationCause {
    /// The failure itself, for a caller that wants more than the verdict.
    #[must_use]
    pub fn error(&self) -> &SqlError {
        &self.0
    }

    /// The SQLSTATE the engine reported, where it reported one.
    #[must_use]
    pub fn sqlstate(&self) -> Option<&'static str> {
        self.0.sqlstate()
    }

    /// Whether applying the migration again is the right thing to do.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.0.is_retryable()
    }
}

impl From<SqlError> for MigrationCause {
    fn from(error: SqlError) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Display for MigrationCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl StdError for MigrationCause {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.0)
    }
}

impl ErrorOverride for MigrationCause {
    /// The verdict alone: full delegation would make the failure the engine's rather than the migration's.
    fn override_is_retryable(&self) -> Option<bool> {
        Some(self.0.is_retryable())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error;
    use fundamental_db::DatabaseError;

    use super::*;

    fn failed(inner: SqlError) -> SqlError {
        SqlError::MigrationFailed {
            name: "0001_probe".to_owned(),
            step: 1,
            total: 1,
            statement: "SELECT 1;".to_owned(),
            cause: inner.into(),
        }
    }

    #[test]
    fn the_verdict_answers_the_same_held_as_a_type_and_held_as_a_dyn() {
        // Without the `#[overrides]` field the trait method would answer `false` here.
        let conflict = failed(SqlError::serialization_conflict("forced"));
        assert!(
            conflict.is_retryable(),
            "the inherent method reads the cause"
        );
        assert!(
            Error::is_retryable(&conflict),
            "and the trait method must agree"
        );
        assert_eq!(
            conflict.sqlstate(),
            Some("40001"),
            "the class is readable, for the label"
        );

        let permanent = failed(SqlError::Config("no such column".to_owned()));
        assert!(!permanent.is_retryable());
        assert!(
            !Error::is_retryable(&permanent),
            "and it must agree in both directions"
        );
        assert_eq!(permanent.sqlstate(), None);
    }

    #[test]
    fn only_the_verdict_is_delegated_and_not_the_variants_identity() {
        let conflict = failed(SqlError::serialization_conflict("forced"));
        assert_eq!(
            conflict.error_code(),
            "SQL_011",
            "not the conflict's SQL_003"
        );
        assert_eq!(conflict.http_status_code(), 500, "not the conflict's 409");
        let rendered = conflict.to_string();
        assert!(
            rendered.contains("0001_probe"),
            "the migration is still named: {rendered}"
        );
        assert!(
            rendered.contains("forced"),
            "and so is what the engine said: {rendered}"
        );
    }

    #[test]
    fn the_verdict_survives_the_hop_a_deploy_actually_holds() {
        // The shape a deploy deciding whether to re-run is holding.
        let wrapped = DatabaseError::from(failed(SqlError::serialization_conflict("forced")));
        assert!(wrapped.is_retryable());
        assert!(!DatabaseError::from(failed(SqlError::Config("bad".to_owned()))).is_retryable());
    }

    #[test]
    fn the_cause_is_a_source_rather_than_a_dead_end() {
        let error = failed(SqlError::serialization_conflict("forced"));
        let source = std::error::Error::source(&error).expect("a migration failure has a cause");
        assert!(source.to_string().contains("forced"), "{source}");
    }
}
