//! The `fun manifest` subcommand: read the project's settings key by key.

mod manifest_args;
mod show;

pub(crate) use manifest_args::{ManifestArgs, run};
