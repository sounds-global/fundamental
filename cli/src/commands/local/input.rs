use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub(crate) struct LocalArgs {
    #[command(subcommand)]
    pub(crate) verb: LocalVerb,
}

#[derive(Debug, Subcommand)]
pub(crate) enum LocalVerb {
    /// Once per machine, with `sudo`: route `.test`, and nothing else, to this machine's resolver
    Setup(SetupArgs),
    /// Once per project, with `sudo`: mint the project's root if needed and trust it in every browser store
    Trust,
    /// Claim this checkout's instance: its address, its names, its certificate
    Up(InstanceArgs),
    /// Show this machine's projects and instances, and whether this one resolves and is trusted
    Status(InstanceArgs),
    /// Print this checkout's instance as `NAME='value'` lines, for `eval`
    Env(EnvArgs),
    /// Give back this checkout's instance: its address, its certificate, its names
    Forget(InstanceArgs),
}

#[derive(Debug, Args)]
pub(crate) struct InstanceArgs {
    /// Serve as the lane with this label (CI names its clone `ci`; a worktree is labelled from its branch)
    #[arg(long, env = "FUN_LOCAL_LABEL")]
    pub(crate) label: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct EnvArgs {
    #[command(flatten)]
    pub(crate) instance: InstanceArgs,
    /// Print only the project, role, label and suffixes, known before anything is claimed
    #[arg(long)]
    pub(crate) names: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SetupArgs {
    /// Print every file `setup` would write as root, and write nothing.
    #[arg(long)]
    pub(crate) print: bool,
}
