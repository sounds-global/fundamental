//! Migration error types.

use super::generate::SchemaDifference;
use super::{LedgerError, LintsError};
use super::{SchemaChange, SchemaDrift};
use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Migration manager errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MigrationError {
    /// An error from a lower layer with no more specific variant.
    Other(AnyError),

    /// IO error.
    #[error(
        code = "MIGRATE_001",
        message = "Something went wrong with the migration: {0}",
        status = 500
    )]
    Io(#[from] std::io::Error),

    /// Migration file parse error.
    #[error(
        code = "MIGRATE_002",
        message = "Parse error in {file}: {message}",
        status = 500
    )]
    Parse {
        /// File path.
        file: String,
        /// Error message.
        message: String,
    },

    /// Migration not found.
    #[error(
        code = "MIGRATE_003",
        message = "Migration not found: {0}",
        status = 404
    )]
    NotFound(String),

    /// Migration already exists.
    #[error(
        code = "MIGRATE_004",
        message = "Migration already exists: {0}",
        status = 409
    )]
    AlreadyExists(String),

    /// Schema verification failed.
    #[error(
        code = "MIGRATE_005",
        message = "Schema verification failed: {message}",
        status = 500
    )]
    SchemaVerification {
        /// Error message.
        message: String,
        /// List of drifts found.
        drifts: Vec<SchemaDrift>,
    },

    /// Database error during migration.
    #[error(code = "MIGRATE_006", message = "database error: {0}", status = 500)]
    Database(String),

    /// Safety check failed.
    #[error(
        code = "MIGRATE_007",
        message = "Safety check failed: {message}. Use --force to override",
        status = 500
    )]
    SafetyCheck {
        /// Error message.
        message: String,
        /// The unsafe changes.
        changes: Vec<SchemaChange>,
    },

    /// Migrations exist on disk that the ledger has no record of.
    ///
    /// An exact comparison of two lists, unlike [`SchemaVerification`](Self::SchemaVerification).
    #[error(code = "MIGRATE_008", message = "{message}", status = 500)]
    UnappliedMigrations {
        /// The full report, including what the drift half could not examine.
        message: String,
        /// The unapplied migration names, in application order.
        names: Vec<String>,
    },

    /// A migration name that is not a single, ordinary path component.
    ///
    /// Refused rather than sanitised, since a name becomes a file path.
    #[error(
        code = "MIGRATE_009",
        message = "{name:?} is not a usable migration name: expected one path component of \
                   [A-Za-z0-9._-]",
        status = 400
    )]
    UnsafeName {
        /// The rejected name, as given.
        name: String,
    },

    /// A staged apply named a migration set the runner was not given.
    ///
    /// Otherwise the apply would do nothing and report success.
    #[error(
        code = "MIGRATE_010",
        message = "the migration set {owner:?} is not registered with this runner",
        status = 500
    )]
    SetNotRegistered {
        /// The owner of the set that was named.
        owner: String,
    },

    /// A replay of the migrations does not make the schema the snapshots and models say, or the
    /// upgrade from the base does not make the schema a replay from scratch makes.
    #[error(code = "MIGRATE_011", message = "{message}", status = 500)]
    Unverified {
        /// The full report: each comparison, and every difference under it.
        message: String,
        /// Every difference found, in report order.
        differences: Vec<SchemaDifference>,
    },

    /// Migrations the base revision holds whose text this branch changed. A database that ran
    /// one refuses it on the next deploy, since an applied file's hash never changes.
    #[error(
        code = "MIGRATE_012",
        message = "migrations changed since {base}: {names:?}; a file a database may have run \
                   never changes, so put the change in a new file",
        status = 409
    )]
    AppliedFileChanged {
        /// Each file's ledger name.
        names: Vec<String>,
        /// The base revision.
        base: String,
    },

    /// New migrations numbered before a file the base revision already holds, in the same set:
    /// a database at the base would apply them after it, in another order than a new one.
    #[error(
        code = "MIGRATE_013",
        message = "new migrations numbered before a file {base} holds: {names:?}; number each \
                   after every file at the base",
        status = 409
    )]
    FileBeforeBase {
        /// Each as `<new file> before <file at base>`.
        names: Vec<String>,
        /// The base revision.
        base: String,
    },

    /// Migrations the base revision holds that this branch removed: a database that ran one
    /// keeps what it made, and a new one never makes it.
    #[error(
        code = "MIGRATE_014",
        message = "migrations {base} holds are gone: {names:?}; a file a database may have run \
                   is never removed",
        status = 409
    )]
    BaseFileGone {
        /// Each file's ledger name.
        names: Vec<String>,
        /// The base revision.
        base: String,
    },

    /// The migrations as the base revision holds them could not be read.
    #[error(
        code = "MIGRATE_015",
        message = "the migrations as at {base} could not be read: {detail}",
        status = 500
    )]
    BaseUnreadable {
        /// The base revision, as given.
        base: String,
        /// What failed.
        detail: String,
    },

    /// The migration ledger refused: an `apply` past a failed migration, or a
    /// `migrate resolve` that cannot be proved or recorded.
    #[error(transparent)]
    Ledger(#[from] LedgerError),

    /// The project's migration lint levels could not be read from its `fun.toml`, so a
    /// migrator built with `lints!()?` does not start.
    #[error(transparent)]
    Lints(#[from] LintsError),
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::MigrationError;

    #[test]
    fn a_file_changed_since_the_base_is_named_with_its_code() {
        let error = MigrationError::AppliedFileChanged {
            names: vec!["newsroom/0007_standfirst".to_owned()],
            base: "3953fc6".to_owned(),
        };
        assert_eq!(error.error_code(), "MIGRATE_012");
        assert_eq!(error.http_status_code(), 409);
        let message = error.to_string();
        assert!(message.contains("newsroom/0007_standfirst"), "{message}");
        assert!(message.contains("3953fc6"), "{message}");
    }

    #[test]
    fn each_refusal_of_the_base_has_its_own_code() {
        let names = || vec!["acme/0002_b".to_owned()];
        let base = || "main".to_owned();
        let codes: Vec<&str> = [
            MigrationError::Unverified {
                message: "1 difference".to_owned(),
                differences: Vec::new(),
            },
            MigrationError::FileBeforeBase {
                names: names(),
                base: base(),
            },
            MigrationError::BaseFileGone {
                names: names(),
                base: base(),
            },
            MigrationError::BaseUnreadable {
                base: base(),
                detail: "not a commit".to_owned(),
            },
        ]
        .iter()
        .map(|error| error.error_code())
        .collect();
        assert_eq!(
            codes,
            ["MIGRATE_011", "MIGRATE_013", "MIGRATE_014", "MIGRATE_015"]
        );
    }
}
