//! Command-line arguments of `fun run` ([`RunArgs`]).

use clap::Args;

/// Arguments accepted by the `fun run` subcommand.
#[derive(Debug, Args)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub(crate) struct RunArgs {
    /// The command to run, as `<namespace>:<name>`; omit it (or pass `--list`) to list them
    pub name: Option<String>,

    /// Arguments forwarded verbatim to the command, after its name
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// List the available commands instead of running one; give `NAME` a namespace to list only it
    #[arg(long, short = 'l')]
    pub list: bool,

    /// With `--list`: build every app's commands binary and check it registers exactly what was read
    #[arg(long, requires = "list")]
    pub verify: bool,

    /// With `--list`: emit the index as JSON instead of the rendered listing.
    #[arg(long, requires = "list")]
    pub json: bool,

    /// The Rust commands binary to run, when the name is no `[commands]` script (default `<app>-commands`, then `commands`)
    #[arg(long)]
    pub bin: Option<String>,

    /// Override the app whose Rust `commands` binary to run (rarely needed: the index knows the owner)
    #[arg(long)]
    pub app: Option<String>,

    /// Build the Rust commands binary with the release profile (a build toggle, not an environment gate)
    #[arg(long)]
    pub release: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Probe {
        #[command(flatten)]
        args: RunArgs,
    }

    #[test]
    fn defaults_to_listing_and_leaves_the_bin_to_the_convention() {
        let probe = Probe::try_parse_from(["fun"]).expect("no args is valid");
        assert_eq!(probe.args.name, None);
        assert!(!probe.args.list);
        assert_eq!(probe.args.bin, None);
        assert!(probe.args.app.is_none());
        assert!(!probe.args.release);
        assert!(!probe.args.verify);
        assert!(!probe.args.json);
        assert!(probe.args.args.is_empty());
    }

    #[test]
    fn captures_an_address() {
        let probe = Probe::try_parse_from(["fun", "mint:operator-token", "--scopes", "a,b"])
            .expect("an address is an ordinary positional");
        assert_eq!(probe.args.name.as_deref(), Some("mint:operator-token"));
        assert_eq!(probe.args.args, vec!["--scopes", "a,b"]);
    }

    #[test]
    fn verify_and_json_need_list() {
        Probe::try_parse_from(["fun", "--verify"]).expect_err("--verify requires --list");
        Probe::try_parse_from(["fun", "--json"]).expect_err("--json requires --list");
        Probe::try_parse_from(["fun", "--list", "--verify", "--json"]).expect("with --list");
    }

    #[test]
    fn captures_the_name_and_passes_through_trailing_flags() {
        let probe =
            Probe::try_parse_from(["fun", "backfill", "--since=2020", "--dry-run"]).expect("valid");
        assert_eq!(probe.args.name.as_deref(), Some("backfill"));
        assert_eq!(probe.args.args, vec!["--since=2020", "--dry-run"]);
    }

    #[test]
    fn accepts_list_app_release_and_bin() {
        let probe = Probe::try_parse_from([
            "fun",
            "--list",
            "--app",
            "billing",
            "--release",
            "--bin",
            "ops",
        ])
        .expect("flags are valid");
        assert!(probe.args.list);
        assert_eq!(probe.args.app.as_deref(), Some("billing"));
        assert!(probe.args.release);
        assert_eq!(probe.args.bin.as_deref(), Some("ops"));
    }

    #[test]
    fn options_before_the_name_are_parsed_then_the_rest_passes_through() {
        let probe =
            Probe::try_parse_from(["fun", "--app", "main", "reindex", "--force"]).expect("valid");
        assert_eq!(probe.args.app.as_deref(), Some("main"));
        assert_eq!(probe.args.name.as_deref(), Some("reindex"));
        assert_eq!(probe.args.args, vec!["--force"]);
    }
}
