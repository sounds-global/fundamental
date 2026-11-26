//! Surface adapters for the framework's transport crates, so a transport never depends on the runtime.

#[cfg(feature = "api")]
mod api_surface;
#[cfg(feature = "ws")]
mod live_sync_surface;
#[cfg(feature = "nats")]
mod nats_surface;
#[cfg(feature = "db")]
mod outbox_pump;
#[cfg(feature = "web")]
mod web_surface;
#[cfg(feature = "ws")]
mod ws_surface;

#[cfg(feature = "ws")]
pub use live_sync_surface::LiveSyncSurface;
#[cfg(feature = "db")]
pub use outbox_pump::OutboxPump;
