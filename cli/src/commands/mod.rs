//! The `fun` subcommands.

pub(super) mod add;
pub(super) mod authz;
pub(super) mod bench;
pub(super) mod build;
pub(super) mod dev;
pub(super) mod external;
pub(super) mod generate;
pub(super) mod keys;
pub(super) mod local;
pub(super) mod manifest;
pub(super) mod messages;
pub(super) mod migrate;
pub(super) mod new;
pub(super) mod package;
pub(super) mod run;
pub(super) mod scaffold;
pub(super) mod seed;
pub(super) mod test;

use std::ffi::OsString;

use clap::Subcommand;

use crate::commands::add::AddArgs;
use crate::commands::authz::AuthzArgs;
use crate::commands::bench::BenchArgs;
use crate::commands::build::BuildArgs;
use crate::commands::dev::DevArgs;
use crate::commands::generate::GenerateArgs;
use crate::commands::keys::KeysArgs;
use crate::commands::local::LocalArgs;
use crate::commands::manifest::ManifestArgs;
use crate::commands::messages::MessagesArgs;
use crate::commands::migrate::MigrateArgs;
use crate::commands::new::NewArgs;
use crate::commands::package::PackageArgs;
use crate::commands::run::RunArgs;
use crate::commands::seed::SeedArgs;
use crate::commands::test::TestArgs;

/// The set of subcommands exposed by the `fun` CLI.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new project
    #[command(visible_alias = "n")]
    New(NewArgs),

    /// Add an app, surface, migrations, or seeds to the current project
    #[command(visible_alias = "a")]
    Add(AddArgs),

    /// Scaffold a domain model (and its migration) into the current project
    #[command(visible_alias = "g")]
    Generate(GenerateArgs),

    /// Create or list the current project's database migrations
    #[command(visible_alias = "m")]
    Migrate(MigrateArgs),

    /// Run the project's database seeds
    #[command(visible_alias = "s")]
    Seed(SeedArgs),

    /// Run a developer command by name (a `[commands]` script or a Rust `#[command]`)
    #[command(visible_alias = "r")]
    Run(RunArgs),

    /// Read the project's settings key by key, with the file and line of each
    Manifest(ManifestArgs),

    /// Run the workspace's tests through nextest's `agent` profile, naming each failure
    Test(TestArgs),

    /// Run the workspace's benchmarks one layer at a time, checked against the committed baseline
    Bench(BenchArgs),

    /// Develop at the project's real `.test` names over HTTPS
    Local(LocalArgs),

    /// Build an app's artifacts (its binary, plus the web bundle if it serves web)
    #[command(visible_alias = "b")]
    Build(BuildArgs),

    /// Build an app and pack what it runs from into one verifiable package (a tar.zst)
    Package(PackageArgs),

    /// Run an app, rebuilding and live-reloading on every change
    #[command(visible_alias = "d")]
    Dev(DevArgs),

    /// Work with the native authorization engine (validate a schema, …)
    Authz(AuthzArgs),

    /// Write the key files a project's configuration reads its keyrings from
    Keys(KeysArgs),

    /// Check each crate's messages: missing translations, stale ones, and unused ones
    Messages(MessagesArgs),

    /// A command `fun` does not have: runs `fun-<name>` from PATH with the words after it
    #[command(external_subcommand)]
    External(Vec<OsString>),
}
