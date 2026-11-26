//! Browser platform layer: persistence through the `sqlite-wasm-vfs` filesystems.
//!
//! IndexedDB databases must be preloaded before opening, which is why opening is async.

use rusqlite::{Connection, OpenFlags};
use sqlite_wasm_rs::WasmOsCallback;
use sqlite_wasm_vfs::{relaxed_idb, sahpool};
use tracing::{debug, instrument};

use crate::db::{OfflineConfig, OfflineDbError, OfflineLocation, WasmBackend};

const IDB_VFS_NAME: &str = "fundamental-relaxed-idb";
const OPFS_VFS_NAME: &str = "fundamental-opfs-sahpool";

/// Handle to the installed VFS management util, kept for export/import.
pub(in crate::db::driver) enum VfsUtil {
    None,
    IndexedDb(relaxed_idb::RelaxedIdbUtil),
    Opfs(sahpool::OpfsSAHPoolUtil),
}

impl std::fmt::Debug for VfsUtil {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "VfsUtil::None",
            Self::IndexedDb(_) => "VfsUtil::IndexedDb",
            Self::Opfs(_) => "VfsUtil::Opfs",
        })
    }
}

/// Per-connection platform state on wasm.
#[derive(Debug)]
pub(in crate::db::driver) struct Platform {
    /// `None` for in-memory databases.
    filename: Option<String>,
    util: VfsUtil,
}

impl Platform {
    pub(in crate::db::driver) fn memory() -> Self {
        Self {
            filename: None,
            util: VfsUtil::None,
        }
    }
}

#[instrument(skip(config), fields(backend = ?config.wasm_backend, db = %name), err(Debug))]
pub(in crate::db::driver) async fn open_persistent(
    config: &OfflineConfig,
    name: &str,
) -> Result<(Connection, Platform), OfflineDbError> {
    let util = install_vfs(config.wasm_backend, name).await?;

    let conn = Connection::open_with_flags_and_vfs(
        name,
        OpenFlags::default(),
        vfs_name(config.wasm_backend),
    )
    .map_err(|e| OfflineDbError::Open(e.to_string()))?;
    debug!("opened persistent wasm database");

    Ok((
        conn,
        Platform {
            filename: Some(name.to_string()),
            util,
        },
    ))
}

#[instrument(skip(_conn, platform), fields(util = ?platform.util), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(in crate::db::driver) fn export_bytes(
    _conn: &Connection,
    platform: &Platform,
) -> Result<Vec<u8>, OfflineDbError> {
    let Some(filename) = &platform.filename else {
        return Err(OfflineDbError::Transfer(
            "cannot export an in-memory database".to_string(),
        ));
    };

    match &platform.util {
        VfsUtil::IndexedDb(util) => util
            .export_db(filename)
            .map_err(|e| OfflineDbError::Transfer(e.to_string())),
        VfsUtil::Opfs(util) => util
            .export_db(filename)
            .map_err(|e| OfflineDbError::Transfer(e.to_string())),
        VfsUtil::None => Err(OfflineDbError::Transfer(
            "no VFS installed for this database".to_string(),
        )),
    }
}

/// Imports SQLite file bytes; must be called before opening.
#[instrument(skip(config, bytes), fields(backend = ?config.wasm_backend, bytes = bytes.len()), err(Debug))]
pub(in crate::db::driver) async fn import_bytes(
    config: &OfflineConfig,
    bytes: &[u8],
) -> Result<(), OfflineDbError> {
    let OfflineLocation::Persistent(name) = &config.location else {
        return Err(OfflineDbError::Transfer(
            "cannot import into an in-memory database".to_string(),
        ));
    };

    match install_vfs(config.wasm_backend, name).await? {
        VfsUtil::IndexedDb(util) => {
            let wait = util
                .import_db(name, bytes)
                .map_err(|e| OfflineDbError::Transfer(e.to_string()))?;
            wait.await
                .map_err(|e| OfflineDbError::Transfer(e.to_string()))
        }
        VfsUtil::Opfs(util) => util
            .import_db(name, bytes)
            .map_err(|e| OfflineDbError::Transfer(e.to_string())),
        VfsUtil::None => unreachable!("install_vfs always returns a util"),
    }
}

fn vfs_name(backend: WasmBackend) -> &'static str {
    match backend {
        WasmBackend::IndexedDb => IDB_VFS_NAME,
        WasmBackend::Opfs => OPFS_VFS_NAME,
    }
}

/// Installs or reuses the backend's VFS and loads the database into memory.
#[instrument(fields(backend = ?backend, db = %name), err(Debug))]
async fn install_vfs(backend: WasmBackend, name: &str) -> Result<VfsUtil, OfflineDbError> {
    match backend {
        WasmBackend::IndexedDb => {
            debug!("installing relaxed-idb VFS and preloading database from IndexedDB");
            let cfg = relaxed_idb::RelaxedIdbCfgBuilder::new()
                .vfs_name(IDB_VFS_NAME)
                .preload(relaxed_idb::Preload::Paths(vec![name.to_string()]))
                .build();
            let util = relaxed_idb::install::<WasmOsCallback>(&cfg, false)
                .await
                .map_err(|e| OfflineDbError::Open(e.to_string()))?;
            // install() skips `preload` when the VFS is already registered.
            util.preload_db(vec![name.to_string()])
                .await
                .map_err(|e| OfflineDbError::Open(e.to_string()))?;
            debug!("explicit IndexedDB preload complete; relaxed-idb VFS ready");
            Ok(VfsUtil::IndexedDb(util))
        }
        WasmBackend::Opfs => {
            debug!("installing OPFS sahpool VFS (dedicated-worker durability)");
            let cfg = sahpool::OpfsSAHPoolCfgBuilder::new()
                .vfs_name(OPFS_VFS_NAME)
                .build();
            let util = sahpool::install::<WasmOsCallback>(&cfg, false)
                .await
                .map_err(|e| OfflineDbError::Open(e.to_string()))?;
            debug!("OPFS sahpool VFS ready");
            Ok(VfsUtil::Opfs(util))
        }
    }
}
