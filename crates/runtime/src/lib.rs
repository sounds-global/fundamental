//! Surface supervision for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! [`Runtime`] starts every [`Surface`], owns the one shutdown signal and drains each surface in
//! registration order; surface errors arrive as one [`RuntimeError`].
//!
//! ```rust
//! # extern crate fundamental_runtime;
//! use fundamental_runtime::Runtime;
//!
//! # async fn run() -> fundamental_runtime::RuntimeResult<()> {
//! // Construct each surface from your app config, then hand them to one runtime:
//! // let worker = Worker::new(client).with_listener(hello);
//! // let socket = WsServer::new(config, router);
//! Runtime::new()
//! //  .with(worker)
//! //  .with(socket)
//!     .run()
//!     .await
//! # }
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

#[cfg(any(
    feature = "nats",
    feature = "ws",
    feature = "api",
    feature = "web",
    feature = "db"
))]
mod adapters;
#[cfg(feature = "api")]
mod admin_error;
#[cfg(feature = "api")]
mod admin_surface;
mod drain_phase;
mod named_surface;
mod outbox_drain;
mod outbox_observer;
mod outbox_surface;
mod periodic_surface;
mod periodic_task;
mod phased_surface;
mod runtime;
mod runtime_error;
mod shutdown_signal;
mod spawned_surface;
mod surface;
mod surface_guard;

#[cfg(feature = "ws")]
pub use adapters::LiveSyncSurface;
#[cfg(feature = "db")]
pub use adapters::OutboxPump;
#[cfg(feature = "api")]
pub use admin_error::AdminError;
#[cfg(feature = "api")]
pub use admin_surface::{AdminSurface, MetricsRenderer};
pub use drain_phase::DrainPhase;
pub use named_surface::NamedSurface;
pub use outbox_drain::OutboxDrain;
pub use outbox_observer::OutboxObserver;
pub use outbox_surface::OutboxSurface;
pub use periodic_surface::PeriodicSurface;
pub use periodic_task::PeriodicTask;
pub use phased_surface::PhasedSurface;
pub use runtime::{LIFECYCLE, Runtime};
pub use runtime_error::{RuntimeError, RuntimeResult};
pub use shutdown_signal::termination_signal;
pub use spawned_surface::SpawnedSurface;
pub use surface::{StartFuture, Surface};
pub use surface_guard::{DrainFuture, SurfaceGuard, WaitFuture};
