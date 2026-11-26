//! Reading a live SQLite schema back into the diff engine's vocabulary.
//!
//! [`OfflineIntrospector`] feeds `fun migrate check`; migrations are hand-written.

#[cfg(not(target_arch = "wasm32"))]
mod introspector;

#[cfg(not(target_arch = "wasm32"))]
pub use introspector::OfflineIntrospector;
