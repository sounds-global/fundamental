//! Arguments of the `fun migrate` family.

use clap::{ArgGroup, Args, Subcommand};

#[derive(Debug, Args)]
pub(crate) struct MigrateArgs {
    #[command(subcommand)]
    pub command: MigrateCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MigrateCommand {
    /// Create a new timestamped migration pair (up + down).
    Create(MigrateCreateArgs),
    /// List the migrations discovered for an app.
    List(MigrateListArgs),
    /// Refuse if a migration on disk is unapplied, and report advisory schema drift.
    Check(MigrateRunArgs),
    /// Apply pending migrations to the live database.
    Apply(MigrateRunArgs),
    /// Show where each migration stands: applied, failed at a step, pending, or waiting
    Status(MigrateRunArgs),
    /// Settle a failed migration with a reason: retry its remaining steps, or record it applied
    Resolve(MigrateResolveArgs),
    /// Replay every migration from scratch and as the upgrade from the base, refusing any drift
    Verify(MigrateVerifyArgs),
    /// Write the next migration of every owner whose models need one (`baseline`: the first snapshot)
    Generate(MigrateGenerateArgs),
    /// Rewrite every owner's migration files as the generator writes them now, for undeployed files
    Regenerate(MigrateRegenerateArgs),
    /// Write this branch's migrations again on top of main's, as `git rebase` replays commits
    Rebase(MigrateRebaseArgs),
}

#[derive(Debug, Args)]
pub(crate) struct MigrateGenerateArgs {
    /// The new file's name after its number, such as `article_standfirst`; `baseline` for the first snapshot
    pub name: String,
    /// The owner crate to generate for; every owner in the workspace when omitted.
    #[arg(long, short = 'p')]
    pub package: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateRegenerateArgs {
    /// The owner crate whose files to rewrite; every owner in the workspace when omitted.
    #[arg(long, short = 'p')]
    pub package: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateRebaseArgs {
    /// The branch to rebase on, such as `release/1.2` (default: the tip of `origin/main`)
    pub upstream: Option<String>,
    /// The owner crate to rebase; every owner in the workspace when omitted.
    #[arg(long, short = 'p')]
    pub package: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateRunArgs {
    /// The app whose `migrate` binary to run.
    #[arg(long)]
    pub app: Option<String>,
    /// The migration binary (a `[[bin]]` using `migrate_main!`; default `<app>-migrate`, then `migrate`)
    #[arg(long)]
    pub bin: Option<String>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("act").required(true).args(["retry", "applied"])))]
pub(crate) struct MigrateResolveArgs {
    /// The failed migration, as `fun migrate status` names it.
    pub name: String,
    /// Run the failed step and every step after it again.
    #[arg(long)]
    pub retry: bool,
    /// Record it applied: refused unless the live catalog shows everything it makes.
    #[arg(long)]
    pub applied: bool,
    /// Why, as the resolution record keeps it.
    #[arg(long)]
    pub reason: String,
    /// The app whose `migrate` binary to run.
    #[arg(long)]
    pub app: Option<String>,
    /// The migration binary name; defaults to `<app>-migrate`, falling back to `migrate`.
    #[arg(long)]
    pub bin: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateVerifyArgs {
    /// The app to verify; every app with a migration binary when omitted.
    #[arg(long)]
    pub app: Option<String>,
    /// The revision a database is upgraded from (default: `git merge-base HEAD origin/main`)
    #[arg(long)]
    pub base: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateCreateArgs {
    /// A short, descriptive name for the migration, e.g. `create_users`.
    pub name: String,
    /// The app to create the migration under.
    #[arg(long, default_value = "main")]
    pub app: String,
    /// The database engine subdirectory, e.g. `sql` or `cql`.
    #[arg(long, default_value = "sql")]
    pub engine: String,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateListArgs {
    /// The app whose migrations should be listed.
    #[arg(long, default_value = "main")]
    pub app: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: MigrateArgs,
    }

    #[test]
    fn verify_parses_with_an_app_and_a_base_or_neither() {
        let probe = Probe::try_parse_from(["fun", "verify", "--app", "ops", "--base", "3953fc6"])
            .expect("verify parses");
        assert!(matches!(
            probe.args.command,
            MigrateCommand::Verify(MigrateVerifyArgs { app: Some(ref a), base: Some(ref b) })
                if a == "ops" && b == "3953fc6"
        ));
        let every = Probe::try_parse_from(["fun", "verify"]).expect("every app");
        assert!(matches!(
            every.args.command,
            MigrateCommand::Verify(MigrateVerifyArgs {
                app: None,
                base: None
            })
        ));
    }

    #[test]
    fn resolve_takes_one_act_and_a_reason() {
        let probe = Probe::try_parse_from([
            "fun",
            "resolve",
            "0007_index_orders",
            "--retry",
            "--reason",
            "timeout at peak",
            "--app",
            "ops",
        ])
        .expect("resolve parses");
        assert!(matches!(
            probe.args.command,
            MigrateCommand::Resolve(MigrateResolveArgs { ref name, retry: true, applied: false, ref reason, app: Some(ref app), .. })
                if name == "0007_index_orders" && reason == "timeout at peak" && app == "ops"
        ));
    }

    #[test]
    fn resolve_without_exactly_one_act_and_a_reason_is_refused() {
        for refused in [
            &["fun", "resolve", "a", "--reason", "why"][..],
            &[
                "fun",
                "resolve",
                "a",
                "--retry",
                "--applied",
                "--reason",
                "why",
            ],
            &["fun", "resolve", "a", "--retry"],
            &["fun", "resolve", "a", "--rolled-back", "--reason", "why"],
            &["fun", "resolve", "--retry", "--reason", "why"],
        ] {
            assert!(Probe::try_parse_from(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn rebase_parses_with_an_upstream_and_an_owner_or_neither() {
        let probe = Probe::try_parse_from(["fun", "rebase", "-p", "newsroom"]).expect("rebase");
        assert!(matches!(
            probe.args.command,
            MigrateCommand::Rebase(MigrateRebaseArgs { upstream: None, package: Some(ref p) })
                if p == "newsroom"
        ));
        let named = Probe::try_parse_from(["fun", "rebase", "release/1.2"]).expect("an upstream");
        assert!(matches!(
            named.args.command,
            MigrateCommand::Rebase(MigrateRebaseArgs { upstream: Some(ref u), package: None })
                if u == "release/1.2"
        ));
        assert!(Probe::try_parse_from(["fun", "rebase", "a", "b"]).is_err());
    }

    #[test]
    fn regenerate_parses_with_one_owner_or_none() {
        let probe = Probe::try_parse_from(["fun", "regenerate", "-p", "portfolio"])
            .expect("regenerate parses");
        assert!(matches!(
            probe.args.command,
            MigrateCommand::Regenerate(MigrateRegenerateArgs { package: Some(ref p) }) if p == "portfolio"
        ));
        let every = Probe::try_parse_from(["fun", "regenerate"]).expect("every owner");
        assert!(matches!(
            every.args.command,
            MigrateCommand::Regenerate(MigrateRegenerateArgs { package: None })
        ));
    }
}
