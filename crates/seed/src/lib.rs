//! Database seeding for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! ```rust
//! # extern crate fundamental_seed;
//! # extern crate fundamental_macros;
//! use fundamental_macros::seed;
//! use fundamental_seed::{SeedRunner, SeedError};
//!
//! // Your application's context — holds whatever providers your seeds need.
//! struct AppContext;
//!
//! #[seed(order = 10)]
//! async fn bootstrap(ctx: &AppContext) -> Result<(), SeedError> {
//!     // ctx.sql().repository::<User>().upsert(&admin).await?;
//!     let _ = ctx;
//!     Ok(())
//! }
//!
//! # async fn run() -> Result<(), SeedError> {
//! let runner = SeedRunner::new().register(bootstrap_seed());
//! runner.run_all(&AppContext).await?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod environment_guard;
mod guard_environment_from;
mod guard_loaded_config;
mod seed;
mod seed_error;
mod seed_runner;

pub use environment_guard::{RELEASE_OVERRIDE_VAR, guard_environment};
pub use guard_environment_from::guard_environment_from;
pub use guard_loaded_config::guard_loaded_config;
pub use seed::{Seed, SeedFn};
pub use seed_error::SeedError;
pub use seed_runner::SeedRunner;
