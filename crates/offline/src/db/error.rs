//! Error type for offline database operations.

use fundamental_macros::Error;

use rusqlite::Error as RusqliteError;

/// Offline database errors.
///
/// Converts into [`fundamental_db::DatabaseError`] on native.
#[derive(Debug, Error)]
#[cfg_attr(not(target_arch = "wasm32"), error_into(fundamental_db::DatabaseError))]
#[non_exhaustive]
pub enum OfflineDbError {
    /// Failed to open the database.
    #[error(
        code = "OFFLINE_001",
        message = "Failed to open offline database: {0}",
        status = 500
    )]
    Open(String),

    /// SQL execution failed.
    #[error(
        code = "OFFLINE_002",
        message = "SQL execution failed: {0}",
        status = 500
    )]
    Execution(String),

    /// A query expected to return rows returned none.
    #[error(code = "OFFLINE_003", message = "Query returned no rows", status = 404)]
    NoRows,

    /// The linked SQLite library is older than the minimum supported version.
    #[error(
        code = "OFFLINE_004",
        message = "SQLite version {found} is older than required {required}",
        status = 500
    )]
    UnsupportedVersion {
        /// The version that was found.
        found: String,
        /// The minimum required version.
        required: String,
    },

    /// Failed to close the database.
    #[error(
        code = "OFFLINE_005",
        message = "Failed to close offline database: {0}",
        status = 500
    )]
    Close(String),

    /// A migration failed to apply.
    #[error(
        code = "OFFLINE_006",
        message = "Migration '{name}' failed: {message}",
        status = 500
    )]
    MigrationFailed {
        /// Migration name.
        name: String,
        /// Error message.
        message: String,
    },

    /// Schema changed for a model that requires data preservation.
    #[error(
        code = "OFFLINE_007",
        message = "Schema changed for table '{table}' (stored version {stored}, model version {current}) but the model requires data preservation; apply a manual migration",
        status = 500
    )]
    SchemaPreserved {
        /// Table name.
        table: String,
        /// Schema version recorded in the database.
        stored: u64,
        /// Schema version of the current model definition.
        current: u64,
    },

    /// A row could not be decoded into a model.
    #[error(
        code = "OFFLINE_008",
        message = "Failed to decode row for {model}: {message}",
        status = 500
    )]
    Decode {
        /// Model type name.
        model: String,
        /// Error message.
        message: String,
    },

    /// Export or import of the database failed.
    #[error(
        code = "OFFLINE_009",
        message = "Database export/import failed: {0}",
        status = 500
    )]
    Transfer(String),

    /// A SQLite operation failed, preserving the typed `rusqlite` error.
    ///
    /// Keeps the [`rusqlite::Error`] as the source, so SQLite's extended result codes survive.
    #[error(
        code = "OFFLINE_010",
        message = "SQLite operation failed",
        status = 500
    )]
    Sqlite(#[from] RusqliteError),

    /// The live schema could not be read back into a comparable form.
    ///
    /// A `PRAGMA` row lacks an expected column, which would make the diff drop a column.
    #[error(
        code = "OFFLINE_011",
        message = "Schema introspection failed: {0}",
        status = 500
    )]
    Introspection(String),
}
