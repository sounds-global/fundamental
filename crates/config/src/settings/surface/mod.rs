//! App and surface settings.

mod app;
mod apps;
mod auth;
mod auth_provider;
mod client;
mod custom;
mod entry;
mod listen_mode;
mod observability;

pub use app::AppConfig;
pub use apps::AppsConfig;
pub use auth::AuthSettings;
pub use custom::CustomSettings;
pub use entry::SurfaceEntryConfig;
pub use listen_mode::ListenMode;

pub use auth_provider::*;
pub use client::*;
pub use observability::*;

/// Deprecated alias for [`AppsConfig`].
#[deprecated(note = "renamed to AppsConfig")]
pub type SurfacesConfig = AppsConfig;

/// Deprecated alias for [`AppConfig`].
#[deprecated(note = "renamed to AppConfig")]
pub type SurfaceGroupConfig = AppConfig;
