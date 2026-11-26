//! Change capture and fan-out: turn a backend's row changes into [`SyncChange`]s
//! and deliver them to live subscribers through a [`ChangePublisher`].

mod capture_table;
mod change_error;
mod change_publisher;
mod sync_change;

pub use capture_table::CaptureTable;
pub use change_error::{ChangeError, ChangeResult};
pub use change_publisher::ChangePublisher;
pub use sync_change::SyncChange;

#[cfg(not(target_arch = "wasm32"))]
mod broadcast_change_publisher;
#[cfg(not(target_arch = "wasm32"))]
mod change_stream;

#[cfg(not(target_arch = "wasm32"))]
pub use broadcast_change_publisher::BroadcastChangePublisher;
#[cfg(not(target_arch = "wasm32"))]
pub use change_stream::ChangeStream;

#[cfg(all(feature = "nats", not(target_arch = "wasm32")))]
mod change_frame;
#[cfg(all(feature = "nats", not(target_arch = "wasm32")))]
mod nats_change_publisher;

#[cfg(all(feature = "nats", not(target_arch = "wasm32")))]
pub use change_frame::ChangeFrame;
#[cfg(all(feature = "nats", not(target_arch = "wasm32")))]
pub use nats_change_publisher::{NatsChangePublisher, nats_change_bridge};
