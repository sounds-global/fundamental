//! The driver failure a migration ended on, kept whole rather than rendered.

use std::error::Error as StdError;
use std::fmt;

use fundamental_core::ErrorOverride;

use crate::CqlError;

/// The failure a migration ended on, carried so that its verdict outlives the runner.
///
/// A value, not text, so a caller can ask whether running the migration again is right; the
/// field holding it is `#[overrides]`, so that verdict survives `dyn Error`.
#[derive(Debug)]
pub struct CqlMigrationCause(Box<CqlError>);

impl CqlMigrationCause {
    /// The failure itself, for a caller that wants more than the verdict.
    #[must_use]
    pub fn error(&self) -> &CqlError {
        &self.0
    }

    /// Whether applying the migration again is the right thing to do.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.0.is_retryable()
    }
}

impl From<CqlError> for CqlMigrationCause {
    fn from(error: CqlError) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Display for CqlMigrationCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl StdError for CqlMigrationCause {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.0)
    }
}

impl ErrorOverride for CqlMigrationCause {
    /// The verdict, and deliberately nothing else.
    ///
    /// Full delegation would report the driver's code and lose the migration as the subject.
    fn override_is_retryable(&self) -> Option<bool> {
        Some(self.0.is_retryable())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error;
    use fundamental_db::DatabaseError;

    use super::*;

    fn failed(inner: CqlError) -> CqlError {
        CqlError::MigrationFailed {
            name: "0001_probe".to_owned(),
            step: 2,
            total: 3,
            statement: "SELECT 1".to_owned(),
            cause: inner.into(),
        }
    }

    #[test]
    fn the_verdict_answers_the_same_held_as_a_type_and_held_as_a_dyn() {
        // `CqlError::timeout` is retryable because the driver's classification says so — this
        // test asserts the projection, not the classification.
        let transient = failed(CqlError::timeout("coordinator did not answer"));
        assert!(
            transient.is_retryable(),
            "the inherent method reads the cause"
        );
        assert!(
            Error::is_retryable(&transient),
            "and the trait method must agree"
        );

        let permanent = failed(CqlError::invalid_query("no such column"));
        assert!(!permanent.is_retryable());
        assert!(
            !Error::is_retryable(&permanent),
            "and it must agree in both directions"
        );
    }

    #[test]
    fn only_the_verdict_is_delegated_and_not_the_variants_identity() {
        let transient = failed(CqlError::timeout("coordinator did not answer"));
        assert_eq!(
            transient.error_code(),
            "CQL_014",
            "not the timeout's CQL_006"
        );
        let rendered = transient.to_string();
        assert!(
            rendered.contains("0001_probe"),
            "the migration is still named: {rendered}"
        );
        assert!(
            rendered.contains("step 2 of 3"),
            "and where it stopped: {rendered}"
        );
    }

    #[test]
    fn an_unrecorded_migration_is_its_own_condition_and_says_what_follows() {
        let unrecorded = CqlError::MigrationRecordFailed {
            name: "0001_probe".to_owned(),
            cause: CqlError::timeout("ledger write timed out").into(),
        };
        assert_eq!(unrecorded.error_code(), "CQL_015");
        assert!(
            unrecorded.is_retryable(),
            "the driver called the ledger write transient"
        );
        assert!(Error::is_retryable(&unrecorded));
        assert!(
            unrecorded.to_string().contains("apply it again"),
            "the consequence belongs in the message, because there is no transaction that \
             undoes the first application: {unrecorded}"
        );
    }

    #[test]
    fn the_verdict_survives_the_hop_a_deploy_actually_holds() {
        let wrapped = DatabaseError::from(failed(CqlError::timeout("no answer")));
        assert!(wrapped.is_retryable());
        assert!(!DatabaseError::from(failed(CqlError::invalid_query("bad"))).is_retryable());
    }

    #[test]
    fn the_cause_is_a_source_rather_than_a_dead_end() {
        let error = failed(CqlError::timeout("coordinator did not answer"));
        let source = std::error::Error::source(&error).expect("a migration failure has a cause");
        assert!(
            source.to_string().contains("coordinator did not answer"),
            "{source}"
        );
    }
}
