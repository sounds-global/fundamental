//! The arguments `fun keys` accepts.

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub(crate) struct KeysArgs {
    #[command(subcommand)]
    pub(crate) command: KeysCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum KeysCommand {
    /// Write a new key version as `secrets/<purpose>/<version>.key` and print the lines that read it
    Mint {
        /// The keyring's purpose, as the code that reads it names it (`acme.session-token.v1`)
        purpose: String,
        /// The version to write; the first is 1
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..))]
        key_version: u16,
    },
}
