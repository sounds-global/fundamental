//! Developer-authored commands for the [Fundamental](https://docs.rs/fundamental) framework:
//! named one-off operations, run one at a time by `fun run <namespace>:<name>`.
//!
//! Mark each command with [`#[command]`](macro@fundamental_macros::command) and register
//! its generated `<fn>_command()` with a [`CommandRunner`] over your own context type:
//!
//! ```rust
//! # extern crate fundamental_command;
//! # extern crate fundamental_macros;
//! use fundamental_macros::command;
//! use fundamental_command::{CommandArgs, CommandError, CommandRunner};
//!
//! // Your application's context — holds whatever providers your commands need.
//! struct AppContext;
//!
//! #[command(namespace = "greet", description = "Say hello")]
//! async fn hello(ctx: &AppContext, args: CommandArgs) -> Result<(), CommandError> {
//!     let _ = (ctx, args);
//!     Ok(())
//! }
//!
//! # async fn run() -> Result<(), CommandError> {
//! let runner = CommandRunner::new().register(hello_command());
//! runner.run("greet:hello", &AppContext, CommandArgs::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [`commands_main!`](macro@fundamental_macros::commands_main) builds a `src/bin/commands.rs`
//! entry point that lists or dispatches them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod command;
mod command_args;
mod command_error;
mod command_listing;
mod command_runner;

pub use command::{Command, CommandFn};
pub use command_args::CommandArgs;
pub use command_error::CommandError;
pub use command_listing::CommandListing;
pub use command_runner::CommandRunner;
