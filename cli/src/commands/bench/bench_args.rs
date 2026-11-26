use clap::{Args, Subcommand};

/// Arguments for `fun bench`. With no layer it counts instructions and checks the baseline.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct BenchArgs {
    /// The layer to run; instruction counts when none is named
    #[command(subcommand)]
    pub(crate) layer: Option<BenchLayer>,

    /// Rewrite this architecture's baseline from the counts instead of checking them; commit it with the reason
    #[arg(long)]
    pub(crate) save: bool,
}

/// One layer of the benchmarks, each a bench target of the same name in a workspace package.
#[derive(Debug, Subcommand)]
pub(crate) enum BenchLayer {
    /// Each hot path's instruction count under callgrind, checked against `benches/baseline/<arch>.json`
    Instructions {
        /// Rewrite the baseline from the counts instead of checking them
        #[arg(long)]
        save: bool,
    },
    /// The same hot paths timed by criterion
    Wall,
    /// The app on a real database and NATS at rising concurrency: throughput and latency percentiles
    Load {
        /// How long each step is measured (`20s`, `1m`)
        #[arg(long, default_value = "20s")]
        each: String,
        /// The concurrency of each step
        #[arg(long, default_value = "1,8,32,128")]
        concurrency: String,
    },
    /// The load mix held for a duration while memory is sampled
    Soak {
        /// How long the soak runs (`10m`, `1h`)
        #[arg(long, default_value = "10m")]
        duration: String,
    },
    /// Writes through a database restart, a NATS outage and a network partition, each found once
    Fault {
        /// How many numbered writes each fault run makes
        #[arg(long, default_value_t = 300)]
        writes: u64,
    },
}

impl BenchLayer {
    pub(crate) fn target(&self) -> &'static str {
        match self {
            Self::Instructions { .. } => "instructions",
            Self::Wall => "wall",
            Self::Load { .. } => "load",
            Self::Soak { .. } => "soak",
            Self::Fault { .. } => "fault",
        }
    }

    /// Whether it drives live engines, and so builds with the `bench-live` profile.
    pub(crate) fn is_live(&self) -> bool {
        matches!(
            self,
            Self::Load { .. } | Self::Soak { .. } | Self::Fault { .. }
        )
    }

    pub(crate) fn program_args(&self) -> Vec<String> {
        match self {
            Self::Instructions { .. } => vec!["--save-summary=json".to_owned()],
            Self::Wall => Vec::new(),
            Self::Load { each, concurrency } => vec![
                "--each".to_owned(),
                each.clone(),
                "--concurrency".to_owned(),
                concurrency.clone(),
            ],
            Self::Soak { duration } => vec!["--duration".to_owned(), duration.clone()],
            Self::Fault { writes } => vec!["--writes".to_owned(), writes.to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Fun {
        #[command(flatten)]
        bench: BenchArgs,
    }

    fn parse(words: &[&str]) -> BenchArgs {
        Fun::try_parse_from(std::iter::once("bench").chain(words.iter().copied()))
            .expect("parses")
            .bench
    }

    #[test]
    fn no_layer_is_the_instruction_counts() {
        let args = parse(&[]);
        assert!(args.layer.is_none());
        assert!(!args.save);
        assert!(parse(&["--save"]).save);
    }

    #[test]
    fn save_before_a_layer_is_refused_rather_than_ignored() {
        let refused = Fun::try_parse_from(["bench", "--save", "wall"]);
        assert!(refused.is_err(), "{refused:?}");
        let instructions = parse(&["instructions", "--save"]).layer.expect("a layer");
        assert!(matches!(
            instructions,
            BenchLayer::Instructions { save: true }
        ));
    }

    #[test]
    fn each_layer_names_its_bench_target() {
        for (word, target) in [
            ("instructions", "instructions"),
            ("wall", "wall"),
            ("load", "load"),
            ("soak", "soak"),
            ("fault", "fault"),
        ] {
            let layer = parse(&[word]).layer.expect("a layer");
            assert_eq!(layer.target(), target);
        }
    }

    #[test]
    fn the_live_layers_pass_their_settings_to_the_program() {
        let load = parse(&["load", "--each", "5s", "--concurrency", "1,8"])
            .layer
            .expect("load");
        assert!(load.is_live());
        assert_eq!(
            load.program_args(),
            ["--each", "5s", "--concurrency", "1,8"]
        );
        let soak = parse(&["soak", "--duration", "10m"]).layer.expect("soak");
        assert_eq!(soak.program_args(), ["--duration", "10m"]);
        let fault = parse(&["fault"]).layer.expect("fault");
        assert_eq!(fault.program_args(), ["--writes", "300"]);
        assert!(!parse(&["wall"]).layer.expect("wall").is_live());
    }
}
