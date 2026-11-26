//! Process telemetry for a Fundamental service: one subscriber, installed once.
//!
//! ```rust,no_run
//! # extern crate fundamental_observability;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (app, _telemetry) = fundamental_observability::boot("layer0")?;
//! // … the guard flushes buffered telemetry when `main` returns.
//! # let _ = app;
//! # Ok(())
//! # }
//! ```
//!
//! [`boot`] loads the configuration under a bootstrap subscriber, so the loader's own lines
//! are kept; [`init`] takes [`AppSettings`] from elsewhere. Metrics are served on the
//! runtime's admin `/metrics`, not here.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod boot;
mod install_panic_hook;
mod nats_connect_ceiling;
mod observability_builder;
mod observability_error;
mod observability_guard;
#[cfg(feature = "otlp")]
mod otlp;
mod request_id;
mod request_id_rejection;
#[cfg(feature = "testing")]
pub mod testing;
pub mod trace_context;

pub use boot::{boot, check_release};
pub use install_panic_hook::install_panic_hook;
pub use nats_connect_ceiling::NatsConnectCeiling;
pub use observability_builder::{BoxedLayer, ObservabilityBuilder, ObservabilitySubscriber};
pub use observability_error::ObservabilityError;
pub use observability_guard::ObservabilityGuard;
pub use request_id::RequestId;
pub use request_id_rejection::RequestIdRejection;

use fundamental_config::AppSettings;

/// Installs this process's telemetry from the app's `[observability]` settings.
///
///
/// # Errors
/// - [`ObservabilityError::AlreadyInitialized`] if a subscriber is already installed.
/// - [`ObservabilityError::Exporter`] if an enabled OTLP exporter cannot be built.
pub fn init(app: &AppSettings) -> Result<ObservabilityGuard, ObservabilityError> {
    ObservabilityBuilder::from_app(app).init()
}
