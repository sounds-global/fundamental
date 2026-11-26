//! Error type for the offline sync engine.

use fundamental_core::AnyError;
use fundamental_db::DatabaseError;
use fundamental_macros::Error;

use crate::wire::SUPPORTED_VERSIONS;

/// Errors produced by the offline sync engine, each with a stable `SYNC_0xx` code.
#[derive(Debug, Error)]
#[error_into(DatabaseError)]
#[non_exhaustive]
pub enum SyncError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// A push (local → server) operation failed.
    #[error(code = "SYNC_001", message = "Sync push failed: {0}", status = 500)]
    Push(String),

    /// A pull (server → local) operation failed.
    #[error(code = "SYNC_002", message = "Sync pull failed: {0}", status = 500)]
    Pull(String),

    /// A conflict could not be resolved automatically.
    #[error(
        code = "SYNC_003",
        message = "Unresolved sync conflict on {table} row {id}",
        status = 409
    )]
    Conflict {
        /// The mirror table the conflict occurred in.
        table: String,
        /// The conflicting row's primary key, rendered as text.
        id: String,
    },

    /// A local mirror storage operation failed.
    #[error(
        code = "SYNC_004",
        message = "Mirror storage failed: {0}",
        status = 500
    )]
    Storage(String),

    /// A wire message was well-formed JSON but carried invalid sync data.
    #[error(
        code = "SYNC_005",
        message = "Malformed sync wire data: {0}",
        status = 422
    )]
    Protocol(String),

    /// A served syncable model has no provisioned server hub table; raised at wiring time.
    #[error(
        code = "SYNC_006",
        message = "no server hub table `{table}` for a served syncable slice: declare the \
                   model's online engine (`#[model(sql, offline, syncable)]` or `cql`) \
                   and add a migration for it",
        status = 500
    )]
    UnprovisionedHub {
        /// The missing base table the slice draws from (`ServerSyncable::TABLE`).
        table: String,
    },

    /// The sender speaks a wire protocol version this build cannot serve; checked before the signature.
    #[error(
        code = "SYNC_007",
        message = "unsupported sync protocol version {version}; this server speaks {supported}",
        status = 426
    )]
    UnsupportedProtocolVersion {
        /// The version the sender claimed.
        version: u32,
        /// The versions this build serves, rendered for the message.
        supported: String,
    },

    /// Slices or projections ask for live change capture, and nothing implements it.
    #[error(
        code = "SYNC_008",
        message = "live change capture is not implemented, but {count} table(s) are \
                   captured: {tables}",
        status = 500
    )]
    NoChangeCapture {
        /// How many tables the merged capture set holds.
        count: usize,
        /// Those tables, rendered for the message.
        tables: String,
    },
}

impl SyncError {
    /// An [`UnsupportedProtocolVersion`](Self::UnsupportedProtocolVersion) naming the versions served.
    #[must_use]
    pub fn unsupported_protocol_version(version: u32) -> Self {
        Self::UnsupportedProtocolVersion {
            version,
            supported: SUPPORTED_VERSIONS
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}
