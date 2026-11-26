//! Offline database abstraction for the Fundamental framework.
//!
//! One SQLite interface on every platform: bundled SQLite natively, `sqlite-wasm-rs`
//! over an IndexedDB or OPFS VFS on `wasm32-unknown-unknown`.
//!
//! Most applications use it through the [`fundamental`] umbrella crate's `offline` feature.
//!
//! [`fundamental`]: https://docs.rs/fundamental

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]

pub mod change;
pub mod db;
pub mod migration;
pub mod runner;
pub mod store;

// Native-only: they offload blocking SQLite work to `spawn_blocking`.
#[cfg(not(target_arch = "wasm32"))]
pub mod query;
#[cfg(not(target_arch = "wasm32"))]
pub mod repository;

#[cfg(not(target_arch = "wasm32"))]
pub use query::OfflineQueryExecutor;
#[cfg(not(target_arch = "wasm32"))]
pub use repository::{OfflineRepository, SharedOfflineDatabase};
