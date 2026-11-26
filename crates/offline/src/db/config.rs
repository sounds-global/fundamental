//! Configuration for opening an offline database.

/// Where the database is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineLocation {
    /// In-memory database; contents are lost when closed.
    Memory,
    /// Persistent database.
    ///
    /// A file path natively; the database name in the browser's [`WasmBackend`].
    Persistent(String),
}

/// Browser storage backend for persistent databases.
///
/// Ignored on native platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WasmBackend {
    /// IndexedDB-backed VFS with relaxed durability; works on the main thread.
    #[default]
    IndexedDb,
    /// OPFS-backed VFS; stronger durability, but only in a dedicated worker.
    Opfs,
}

/// Configuration for opening an offline database.
#[derive(Debug, Clone)]
pub struct OfflineConfig {
    /// Where the database is stored.
    pub location: OfflineLocation,
    /// Whether to enable foreign key enforcement (`PRAGMA foreign_keys=ON`).
    pub foreign_keys: bool,
    /// Browser storage backend (ignored on native platforms).
    pub wasm_backend: WasmBackend,
}

impl OfflineConfig {
    /// Configuration for an in-memory database.
    pub fn memory() -> Self {
        Self {
            location: OfflineLocation::Memory,
            foreign_keys: true,
            wasm_backend: WasmBackend::default(),
        }
    }

    /// Configuration for a persistent database.
    pub fn persistent(name: impl Into<String>) -> Self {
        Self {
            location: OfflineLocation::Persistent(name.into()),
            foreign_keys: true,
            wasm_backend: WasmBackend::default(),
        }
    }

    /// Sets the browser storage backend.
    pub fn with_wasm_backend(mut self, backend: WasmBackend) -> Self {
        self.wasm_backend = backend;
        self
    }

    /// Disables foreign key enforcement.
    pub fn without_foreign_keys(mut self) -> Self {
        self.foreign_keys = false;
        self
    }
}
