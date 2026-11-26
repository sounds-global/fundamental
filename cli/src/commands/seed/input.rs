//! Command-line arguments of `fun seed` ([`SeedArgs`]).

use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct SeedArgs {
    /// Name of the seeder binary to run (a project `[[bin]]` using `seed_main!`)
    #[arg(long, default_value = "seed")]
    pub bin: String,

    /// The app whose seeder to run (default: the sole app that defines `seeds/`)
    #[arg(long)]
    pub app: Option<String>,

    /// Allow seeding a deployed environment (staging or production); the seeder enforces it too
    #[arg(long)]
    pub release: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: SeedArgs,
    }

    #[test]
    fn defaults_to_the_seed_bin_without_release_or_app() {
        let probe = Probe::try_parse_from(["fun"]).expect("no args is valid");
        assert_eq!(probe.args.bin, "seed");
        assert!(!probe.args.release);
        assert_eq!(probe.args.app, None);
    }

    #[test]
    fn accepts_a_custom_bin_app_and_release_flag() {
        let probe = Probe::try_parse_from([
            "fun",
            "--bin",
            "demo-seeder",
            "--app",
            "billing",
            "--release",
        ])
        .expect("flags are valid");
        assert_eq!(probe.args.bin, "demo-seeder");
        assert_eq!(probe.args.app.as_deref(), Some("billing"));
        assert!(probe.args.release);
    }
}
