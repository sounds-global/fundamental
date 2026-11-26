//! Command-line arguments of `fun new` ([`NewArgs`]).

use std::path::PathBuf;

use clap::Args;

use crate::Surface;

#[derive(Debug, Args)]
pub(crate) struct NewArgs {
    /// Name of the project to create (prompted for when omitted)
    pub name: Option<String>,
    /// Path to the project directory
    pub path: Option<PathBuf>,
    /// Surfaces to generate, e.g. `--surfaces worker,socket`; an empty list makes a domain-only library
    #[arg(long, value_delimiter = ',')]
    pub surfaces: Option<Vec<Surface>>,
    /// Name of the app to create when surfaces are selected (default `main`)
    #[arg(long)]
    pub app: Option<String>,
    /// Port for the socket surface (default 9000)
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
    /// Skip all interactive prompts, taking defaults (no `--surfaces`: a domain-only library)
    #[arg(long)]
    pub yes: bool,
}
