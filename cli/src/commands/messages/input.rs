//! The arguments `fun messages` accepts.

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub(crate) struct MessagesArgs {
    #[command(subcommand)]
    pub(crate) command: MessagesCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MessagesCommand {
    /// Report each crate's missing, stale and unused messages per language, failing as `[messages]` says
    Check {
        /// Only this crate
        #[arg(short, long)]
        package: Option<String>,
        /// Judge as CI does, whether or not `CI` is set
        #[arg(long)]
        ci: bool,
    },
}
