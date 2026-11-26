//! Per-platform driver pieces: opening persistent databases and export/import.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::{Platform, export_bytes, import_bytes, open_persistent};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(super) use wasm::{Platform, export_bytes, import_bytes, open_persistent};
