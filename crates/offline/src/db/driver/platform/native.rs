//! Native platform layer: filesystem-backed SQLite.

use rusqlite::Connection;
use tracing::warn;

use crate::db::{OfflineConfig, OfflineDbError, OfflineLocation};

#[derive(Debug)]
pub(in crate::db::driver) struct Platform;

impl Platform {
    pub(in crate::db::driver) fn memory() -> Self {
        Self
    }
}

pub(in crate::db::driver) async fn open_persistent(
    _config: &OfflineConfig,
    path: &str,
) -> Result<(Connection, Platform), OfflineDbError> {
    let conn = Connection::open(path).map_err(|e| OfflineDbError::Open(e.to_string()))?;
    Ok((conn, Platform))
}

/// Exports the database as SQLite file bytes via `VACUUM INTO`.
pub(in crate::db::driver) fn export_bytes(
    conn: &Connection,
    _platform: &Platform,
) -> Result<Vec<u8>, OfflineDbError> {
    let path = std::env::temp_dir().join(format!(
        "fundamental_offline_export_{}_{:?}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let path_str = path.to_string_lossy().into_owned();

    conn.execute("VACUUM INTO ?1", [&path_str])
        .map_err(|e| OfflineDbError::Transfer(e.to_string()))?;

    let bytes = std::fs::read(&path).map_err(|e| OfflineDbError::Transfer(e.to_string()));
    if let Err(error) = std::fs::remove_file(&path) {
        warn!(
            path = %path.display(),
            %error,
            "failed to remove offline export temp file; it may be orphaned"
        );
    }
    bytes
}

/// Imports SQLite file bytes; must be called before opening.
pub(in crate::db::driver) async fn import_bytes(
    config: &OfflineConfig,
    bytes: &[u8],
) -> Result<(), OfflineDbError> {
    match &config.location {
        OfflineLocation::Memory => Err(OfflineDbError::Transfer(
            "cannot import into an in-memory database".to_string(),
        )),
        OfflineLocation::Persistent(path) => {
            std::fs::write(path, bytes).map_err(|e| OfflineDbError::Transfer(e.to_string()))
        }
    }
}
