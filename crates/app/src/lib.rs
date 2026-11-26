//! One [`App`] value per app: its pages, its look, its state and every surface it runs.
//!
//! ```
//! # extern crate fundamental_app;
//! # extern crate fundamental_server;
//! use fundamental_server::{HttpContext, Route, Router, ServerError};
//! use fundamental_app::App;
//!
//! fn register_pages(router: Router<()>) -> Result<Router<()>, ServerError> {
//!     router.route(Route::html("/", |_: HttpContext<()>| async { Ok("<h1>Hi</h1>".to_owned()) }))
//! }
//!
//! /// The app, as `main.rs` runs it and the tests answer it.
//! pub fn app() -> App<()> {
//!     fundamental_app::app!().pages(register_pages).state(|_| Ok(()))
//! }
//!
//! // main.rs: fn main() -> std::process::ExitCode { www::app().run() }
//! # assert_eq!(app().router()?.page_paths(), ["/"]);
//! # Ok::<(), fundamental_core::AnyError>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod app;
mod app_error;
mod app_part;
mod declared;
#[cfg(any(feature = "worker", feature = "sync"))]
mod deferred;
mod launch;
mod part_surface;
mod served;
#[cfg(feature = "socket")]
mod socket;
#[cfg(feature = "sync")]
mod sync;
mod unlisted;
#[cfg(feature = "worker")]
mod worker;

pub use app::App;
pub use app_error::AppError;
pub use app_part::AppPart;
pub use served::Served;
#[cfg(feature = "sync")]
pub use sync::SyncStore;

/// [`App::new`] for the calling crate, named by its package.
///
/// ```
/// # extern crate fundamental_app;
/// let app: fundamental_app::App<()> = fundamental_app::app!();
/// assert_eq!(app.name(), "fundamental_app");
/// ```
#[macro_export]
macro_rules! app {
    () => {
        $crate::App::new(env!("CARGO_PKG_NAME"))
            .built(concat!(env!("CARGO_MANIFEST_DIR"), "/dist"))
            .version(env!("CARGO_PKG_VERSION"))
    };
}
