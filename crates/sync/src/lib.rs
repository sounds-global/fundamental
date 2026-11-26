//! # Offline sync for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//!
//! [`mirror_table_schema`] derives the offline mirror table of a `#[model(syncable)]` model: its
//! synced columns plus the overlay (`updated_at`, `version`, `_tombstone`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod adapters;
pub mod change;
pub mod engine;
pub mod wire;

mod memory_slice_store;
mod mirror_schema;
mod no_params;
mod server_syncable;
mod single_key_syncable;
mod slice;
mod slice_bind_error;
mod slice_match;
mod slice_reader;
mod slice_store;
mod sync_column;
mod sync_column_role;
mod sync_error;
mod syncable;
mod vault_error;

#[cfg(not(target_arch = "wasm32"))]
pub use change::{BroadcastChangePublisher, ChangeStream};
pub use change::{CaptureTable, ChangeError, ChangePublisher, ChangeResult, SyncChange};
#[cfg(all(feature = "nats", not(target_arch = "wasm32")))]
pub use change::{ChangeFrame, NatsChangePublisher, nats_change_bridge};
pub use engine::{
    LocalMirror, PullOutcome, RemoteSource, Resolution, SliceBinding, SyncEngine, SyncMeta,
    SyncRecord, resolve_conflict,
};
pub use fundamental_core::model::offline::ConflictStrategy;
pub use memory_slice_store::MemorySliceStore;
pub use mirror_schema::{ensure_mirror_table, mirror_table_schema};
pub use no_params::NoParams;
pub use server_syncable::ServerSyncable;
pub use single_key_syncable::SingleKeySyncable;
pub use slice::Slice;
pub use slice_bind_error::SliceBindError;
pub use slice_match::record_matches;
pub use slice_reader::SliceReader;
pub use slice_store::{SliceStore, SliceWriteStore};
pub use sync_column::SyncColumn;
pub use sync_column_role::SyncColumnRole;
pub use sync_error::SyncError;
pub use syncable::Syncable;
pub use vault_error::VaultError;
