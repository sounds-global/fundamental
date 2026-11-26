//! `fun`: the developer CLI for the Fundamental framework. Run `fun --help` for its commands.
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rustdoc::all)]
// A binary's items are crate-private, and its docs are built with private items.
#![allow(rustdoc::private_intra_doc_links)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod cli;
mod commands;
mod context;
mod emit;
mod engine;
mod error;
mod helpers;
mod prompt_theme;
mod result;
mod surface;
mod ui;
mod version;

use clap::Parser;
use fundamental::core::error::Error as _;
use std::io::IsTerminal;
use tracing_subscriber::EnvFilter;

use cli::Cli;
use commands::Command;
use context::CommandContext;
use error::CliError;
use result::CliResult;
use surface::Surface;
use ui::Ui;

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose, cli.quiet);
    let quiet = cli.quiet;

    if let Err(err) = cli.run() {
        Ui::new(quiet).failure(&err);
        std::process::exit(err.exit_code());
    }
}

/// Installs the `tracing` subscriber on stderr: `RUST_LOG`, else the level the global flags give.
fn init_tracing(verbose: bool, quiet: bool) {
    let default_level = if quiet {
        "error"
    } else if verbose {
        "debug"
    } else {
        "warn"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        // `fmt()` turns ANSI on unconditionally, so a piped run would carry escape codes.
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}
