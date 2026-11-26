//! Live reactive reads for the web surface: [`use_live`] binds a slice to a Leptos signal.

mod host_sync_config;
#[cfg(all(target_arch = "wasm32", feature = "tauri"))]
mod hosted;
mod live_context;
mod provide_live_hosted;
mod provide_live_lazy;
mod use_live;
mod use_live_one;

pub use host_sync_config::host_sync_config;
pub use live_context::{LiveContext, provide_live};
pub use provide_live_hosted::provide_live_hosted;
pub use provide_live_lazy::provide_live_lazy;
pub use use_live::{use_live, use_live_with};
pub use use_live_one::use_live_one;
