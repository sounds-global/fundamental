//! Arguments of the `fun add` family.

use clap::{Args, Subcommand};

use crate::Surface;

#[derive(Debug, Args)]
pub(crate) struct AddArgs {
    #[command(subcommand)]
    pub command: AddCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AddCommand {
    /// Add a new app crate and register it in `fun.toml`.
    App(AddAppArgs),
    /// Add a runtime surface to an existing app.
    Surface(AddSurfaceArgs),
    /// Create a `migrations/<engine>/` directory with a sample first migration.
    Migrations(AddMigrationsArgs),
    /// Create a `seeds/` directory with a sample `#[seed]` stub.
    Seeds(AddSeedsArgs),
    /// Scaffold a Rust `commands` binary with a sample `#[command]` for `fun run`.
    Commands(AddCommandsArgs),
    /// Add a `[commands]` shell script to `fun.toml` for `fun run`.
    Command(AddCommandArgs),
    /// Scaffold an authorization schema (`#[derive(Namespace)]`) and its wiring.
    Authz(AddAuthzArgs),
}

#[derive(Debug, Args)]
pub(crate) struct AddAppArgs {
    /// Name of the app to create (also its crate name, sanitized)
    pub name: String,
    /// Surfaces to generate, e.g. `--surfaces worker,socket` (skips the interactive selection)
    #[arg(long, value_delimiter = ',')]
    pub surfaces: Option<Vec<Surface>>,
    /// Port for the socket surface (default: a stable per-app offset from 9000)
    #[arg(long)]
    pub port: Option<u16>,
    /// Database engines to wire up, comma-separated: `none`, `sql`, `cql`, or `sql,cql`
    #[arg(long, value_delimiter = ',')]
    pub db: Option<Vec<String>>,
    /// Which backend backs the `sync` surface's slice store when both are wired: `sql` or `cql`
    #[arg(long)]
    pub sync_store: Option<String>,
    /// Set up Tailwind CSS for the web surface
    #[arg(long)]
    pub tailwind: bool,
    /// The domain crate the app depends on, when the project has several under `domain/`
    #[arg(long, value_name = "CRATE")]
    pub domain: Option<String>,
    /// Skip all interactive prompts, taking defaults
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub(crate) struct AddSurfaceArgs {
    /// The surface kind to add: `worker` or `socket`.
    pub kind: Surface,
    /// The app to add the surface to.
    #[arg(long, default_value = "main")]
    pub app: String,
    /// For the `web` surface: wire up Tailwind CSS
    #[arg(long)]
    pub tailwind: bool,
    /// For the `web` surface: the domain crate it depends on, when the project has several
    #[arg(long, value_name = "CRATE")]
    pub domain: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct AddMigrationsArgs {
    /// The app to add the migrations directory to.
    #[arg(long, default_value = "main")]
    pub app: String,
    /// The database engine subdirectory, e.g. `sql` or `cql`.
    #[arg(long, default_value = "sql")]
    pub engine: String,
}

#[derive(Debug, Args)]
pub(crate) struct AddSeedsArgs {
    /// The app to add the seeds directory to.
    #[arg(long, default_value = "main")]
    pub app: String,
}

#[derive(Debug, Args)]
pub(crate) struct AddAuthzArgs {
    /// The app to add the authorization module to.
    #[arg(long, default_value = "main")]
    pub app: String,
}

#[derive(Debug, Args)]
pub(crate) struct AddCommandsArgs {
    /// The app to add the `commands` binary to.
    #[arg(long, default_value = "main")]
    pub app: String,
}

#[derive(Debug, Args)]
pub(crate) struct AddCommandArgs {
    /// The command's address, `<namespace>:<name>`
    pub name: String,
    /// The shell command line to run, e.g. `-- cargo clippy --all-targets`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub run: Vec<String>,
}
