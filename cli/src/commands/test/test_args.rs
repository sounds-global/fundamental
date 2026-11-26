use clap::Args;

/// Arguments for `fun test`: the change's reach, the filters, or with `--all` everything.
#[derive(Debug, Args)]
pub(crate) struct TestArgs {
    /// Nextest filter expressions, run as their union (default: the packages the change reaches)
    pub(crate) filters: Vec<String>,

    /// Run every test in the workspace, whatever changed
    #[arg(long, conflicts_with = "filters")]
    pub(crate) all: bool,

    /// Build with the lockfile as it is, as `cargo --locked` does
    #[arg(long)]
    pub(crate) locked: bool,
}
