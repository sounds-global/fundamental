//! `fun bench`: runs one layer's bench targets, and checks or saves the instruction counts.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use fundamental_bench::{Baseline, BenchError, Measured};
use tracing::{info, instrument};

use super::bench_args::{BenchArgs, BenchLayer};
use super::bench_target::BenchTarget;
use super::container::{Container, answers};

use crate::commands::test::metadata;
use crate::helpers::Project;
use crate::{CliError, CliResult, CommandContext};

/// The profile the live layers build with: `release` with debug assertions on.
pub(crate) const LIVE_PROFILE: &str = "bench-live";

const BASELINE_DIR: &str = "benches/baseline";

/// Runs every member's bench target of the layer `args` names (instruction counts by default).
///
/// # Errors
/// [`CliError::BenchDidNotRun`], [`CliError::BenchFailed`] or [`CliError::BenchRegressed`].
#[expect(
    clippy::cognitive_complexity,
    reason = "the span expands into branches"
)]
#[instrument(skip_all, fields(layer = ?args.layer, save = args.save), err(Debug))]
pub(crate) fn run(args: BenchArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let root = project.root();
    let layer = args
        .layer
        .unwrap_or(BenchLayer::Instructions { save: args.save });
    let save = args.save || matches!(layer, BenchLayer::Instructions { save: true });
    let metadata = metadata(root, &["--no-deps"])?;
    let target_dir = metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| did_not_run("cargo metadata names no target directory".to_owned()))?;
    let targets = BenchTarget::all_named(&metadata, layer.target());
    if targets.is_empty() {
        return Err(did_not_run(format!(
            "no workspace package has a bench target named `{0}`; add `benches/{0}.rs` (harness = false) to the package that benches the app",
            layer.target()
        )));
    }
    match layer {
        BenchLayer::Instructions { .. } => instructions(root, &target_dir, &targets, save, &ctx),
        _ => programs(root, &target_dir, &targets, &layer, &ctx),
    }
}

fn programs(
    root: &Path,
    target_dir: &Path,
    targets: &[BenchTarget],
    layer: &BenchLayer,
    ctx: &CommandContext,
) -> CliResult<()> {
    if layer.is_live() && !declares_live_profile(root) {
        return Err(did_not_run(format!(
            "the workspace's Cargo.toml declares no `[profile.{LIVE_PROFILE}]`; add `[profile.{LIVE_PROFILE}]` with `inherits = \"release\"` and `debug-assertions = true`, which the test containers' fixtures need"
        )));
    }
    for target in targets {
        ctx.ui()
            .section(format!("{} in {}", layer.target(), target.package));
        let mut cargo = Command::new("cargo");
        cargo
            .current_dir(root)
            .arg("bench")
            .args(target.cargo_args(layer.target()));
        if layer.is_live() {
            cargo.args(["--profile", LIVE_PROFILE]);
        }
        let status = cargo
            .arg("--")
            .args(layer.program_args())
            .env("BENCH_OUT", target_dir.join("bench"))
            .status()
            .map_err(|error| did_not_run(format!("cargo bench cannot be run: {error}")))?;
        info!(package = %target.package, layer = layer.target(), %status, "fun.bench.ran");
        if !status.success() {
            return Err(CliError::BenchFailed {
                layer: layer.target().to_owned(),
                package: target.package.clone(),
            });
        }
    }
    ctx.ui().note(format!(
        "the reports are in {}",
        target_dir.join("bench").display()
    ));
    Ok(())
}

/// Counts every `instructions` target, then checks the counts against the baseline or saves them.
fn instructions(
    root: &Path,
    target_dir: &Path,
    targets: &[BenchTarget],
    save: bool,
    ctx: &CommandContext,
) -> CliResult<()> {
    let summaries = match Counter::choose(
        Container::find(root),
        answers("valgrind", &["--version"]),
    ) {
        Some(Counter::Container(container)) => {
            count_in_container(root, target_dir, targets, &container, ctx)?
        }
        Some(Counter::Native) => {
            ctx.ui().note(
                "no docker or podman here: counting with this machine's valgrind, whose libc and valgrind may differ from the baseline's container",
            );
            count_natively(root, target_dir, targets)?
        }
        None => {
            return Err(did_not_run(
                "this machine has neither docker nor podman to count in the pinned container, nor valgrind to count natively; install a container engine".to_owned(),
            ));
        }
    };
    let measured = Measured::read_summaries(&summaries).map_err(bench_error)?;
    if measured.is_empty() {
        return Err(did_not_run(format!(
            "the benches wrote no summary under {}",
            summaries.display()
        )));
    }
    let dir = root.join(BASELINE_DIR);
    if save {
        let baseline = match Baseline::read(&dir) {
            Ok(baseline) => baseline,
            Err(BenchError::NoBaseline { .. }) => Baseline::new(),
            Err(error) => return Err(bench_error(error)),
        };
        let mut saved = baseline.saved(&measured);
        if let Some(toolchain) = rustc_version() {
            saved = saved.with_toolchain(toolchain);
        }
        let path = saved.write(&dir).map_err(bench_error)?;
        ctx.ui().note(format!(
            "{} counts saved to {}; commit it with the reason the counts moved",
            measured.len(),
            path.display()
        ));
        return Ok(());
    }
    let checked = Baseline::read(&dir)
        .map_err(bench_error)?
        .check(&measured)
        .map_err(|error| CliError::BenchRegressed {
            reason: error.to_string(),
        })?;
    ctx.ui()
        .note(format!("{} benches within the baseline", checked.compared));
    if !checked.improved.is_empty() {
        ctx.ui().note(format!(
            "faster than the baseline: {}; `fun bench --save` keeps the gain",
            checked.improved.join(", ")
        ));
    }
    Ok(())
}

/// Runs with this machine's valgrind; answers where gungraun wrote the summaries.
fn count_natively(root: &Path, target_dir: &Path, targets: &[BenchTarget]) -> CliResult<PathBuf> {
    if !answers("gungraun-runner", &["--version"]) {
        return Err(did_not_run(
            "valgrind is here and gungraun-runner is not; install the runner of the version the lockfile names (cargo install gungraun-runner --version <it>)".to_owned(),
        ));
    }
    let summaries = target_dir.join("gungraun");
    clear(&summaries)?;
    for target in targets {
        let status = Command::new("cargo")
            .current_dir(root)
            .arg("bench")
            .args(target.cargo_args("instructions"))
            .args(["--", "--save-summary=json"])
            .status()
            .map_err(|error| did_not_run(format!("cargo bench cannot be run: {error}")))?;
        if !status.success() {
            return Err(CliError::BenchFailed {
                layer: "instructions".to_owned(),
                package: target.package.clone(),
            });
        }
    }
    Ok(summaries)
}

#[derive(Debug, PartialEq, Eq)]
enum Counter {
    /// In the pinned container, whose toolchain, libc and valgrind are the baseline's.
    Container(Container),
    Native,
}

impl Counter {
    /// The container whenever an engine is here; only its counts compare with a baseline taken in it.
    fn choose(container: Option<Container>, valgrind: bool) -> Option<Self> {
        match container {
            Some(container) => Some(Self::Container(container)),
            None if valgrind => Some(Self::Native),
            None => None,
        }
    }
}

/// Runs in the pinned container; answers where gungraun wrote the summaries.
fn count_in_container(
    root: &Path,
    target_dir: &Path,
    targets: &[BenchTarget],
    container: &Container,
    ctx: &CommandContext,
) -> CliResult<PathBuf> {
    ctx.ui()
        .note(format!("counting in the container {}", container.image()));
    container.ensure_image()?;
    let built = target_dir.join("bench-container");
    let summaries = built.join("target").join("gungraun");
    clear(&summaries)?;
    fs::create_dir_all(built.join("cargo"))
        .map_err(|error| did_not_run(format!("{}: {error}", built.display())))?;
    for target in targets {
        let mut cargo = target.cargo_args("instructions");
        cargo.extend(["--".to_owned(), "--save-summary=json".to_owned()]);
        let status = container
            .command(root, &built, &cargo)
            .status()
            .map_err(|error| did_not_run(format!("the container cannot be run: {error}")))?;
        if !status.success() {
            return Err(CliError::BenchFailed {
                layer: "instructions".to_owned(),
                package: target.package.clone(),
            });
        }
    }
    Ok(summaries)
}

/// So a bench since removed is not counted.
fn clear(summaries: &Path) -> CliResult<()> {
    match fs::remove_dir_all(summaries) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(did_not_run(format!(
            "the last run's summaries, {}, cannot be removed: {error}",
            summaries.display()
        ))),
        _ => Ok(()),
    }
}

fn declares_live_profile(root: &Path) -> bool {
    fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .is_some_and(|manifest| {
            manifest
                .get("profile")
                .and_then(|profiles| profiles.get(LIVE_PROFILE))
                .is_some()
        })
}

fn rustc_version() -> Option<String> {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn bench_error(error: BenchError) -> CliError {
    did_not_run(error.to_string())
}

fn did_not_run(reason: String) -> CliError {
    CliError::BenchDidNotRun { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_profile_is_found_in_the_workspace_manifest() {
        let dir = tempfile::tempdir().expect("a directory");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\n\n[profile.bench-live]\ninherits = \"release\"\ndebug-assertions = true\n",
        )
        .expect("written");
        assert!(declares_live_profile(dir.path()));
        fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").expect("written");
        assert!(!declares_live_profile(dir.path()));
    }

    #[test]
    fn the_counts_are_taken_in_the_container_whenever_an_engine_is_here() {
        assert_eq!(
            Counter::choose(Some(Container::example()), true),
            Some(Counter::Container(Container::example()))
        );
        assert_eq!(Counter::choose(None, true), Some(Counter::Native));
        assert_eq!(Counter::choose(None, false), None);
    }

    #[test]
    fn clearing_summaries_that_are_not_there_is_fine() {
        let dir = tempfile::tempdir().expect("a directory");
        clear(&dir.path().join("gungraun")).expect("nothing to clear");
    }
}
