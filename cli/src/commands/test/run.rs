//! `fun test`: the one way an agent runs the tests and reads what failed.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{info, instrument, warn};

use super::TestArgs;
use super::selection::Selection;
use super::test_failure::TestFailure;
use super::workspace::Workspace;

use crate::helpers::Project;
use crate::{CliError, CliResult, CommandContext};

/// Declared in `.config/nextest.toml` with `fail-fast = false` and a JUnit report.
pub(crate) const PROFILE: &str = "agent";

/// Run the tests nextest's [`PROFILE`] selects, then name each failure from the JUnit report.
///
/// Always `--workspace`, narrowed by filters (never `-p`), so every run reuses one build.
///
/// # Errors
/// [`CliError::TestsDidNotRun`] without a readable report, [`CliError::TestsFailed`] on a failure.
#[instrument(skip_all, fields(filters = ?args.filters), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn run(args: TestArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    test(project.root(), &args, &ctx)
}

pub(crate) fn test(root: &Path, args: &TestArgs, ctx: &CommandContext) -> CliResult<()> {
    let Some(filters) = what_runs(root, &args.filters, args.all, ctx)? else {
        return Ok(());
    };
    let report = report_path(root)?;
    match fs::remove_file(&report) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(did_not_run(format!(
                "the last report, {}, cannot be removed: {error}",
                report.display()
            )));
        }
        _ => {}
    }
    let status = nextest(root, &filters, args)
        .status()
        .map_err(|error| did_not_run(format!("cargo nextest cannot be run: {error}")))?;
    let text = fs::read_to_string(&report).map_err(|error| {
        warn!(%status, report = %report.display(), %error, "fun.test.no_report");
        did_not_run(format!(
            "nextest exited {status} and left no report at {} ({error}): the tests did not build, or `[profile.{PROFILE}.junit]` in .config/nextest.toml names no `path = \"junit.xml\"`",
            report.display()
        ))
    })?;
    let failures = TestFailure::all_in(&text).map_err(|reason| {
        did_not_run(format!(
            "the report {} is not JUnit: {reason}",
            report.display()
        ))
    })?;
    for failure in &failures {
        ctx.ui().section(format!("failed: {}", failure.name));
        ctx.ui().detail(&failure.said);
    }
    info!(failed = failures.len(), %status, report = %report.display(), "fun.test.done");
    match (failures.len(), status.success()) {
        (0, true) => Ok(()),
        (0, false) => Err(did_not_run(format!(
            "nextest exited {status} and its report names no failed test; read {}",
            report.display()
        ))),
        (failed, _) => Err(CliError::TestsFailed {
            failed,
            report: report.display().to_string(),
        }),
    }
}

fn nextest(root: &Path, filters: &[String], args: &TestArgs) -> Command {
    let mut nextest = Command::new("cargo");
    nextest.current_dir(root).args([
        "nextest",
        "run",
        "--workspace",
        "--profile",
        PROFILE,
        "--no-fail-fast",
    ]);
    if args.locked {
        nextest.arg("--locked");
    }
    for filter in filters {
        nextest.args(["-E", filter]);
    }
    nextest
}

/// The filters this run passes nextest, or `None` when the change reaches no test to run.
fn what_runs(
    root: &Path,
    filters: &[String],
    all: bool,
    ctx: &CommandContext,
) -> CliResult<Option<Vec<String>>> {
    if all || !filters.is_empty() {
        return Ok(Some(filters.to_vec()));
    }
    reached(root, ctx)
}

/// Filters for the packages the change reaches (none for everything); `None` when it reaches none.
fn reached(root: &Path, ctx: &CommandContext) -> CliResult<Option<Vec<String>>> {
    let workspace = Workspace::from_metadata(&metadata(root, &[])?)
        .ok_or_else(|| did_not_run("cargo metadata names no workspace and packages".to_owned()))?;
    match workspace.affected()? {
        Selection::Everything => {
            ctx.ui().note(
                "the change reaches a workspace file no package declares it reads (`[package.metadata.fun] reads`), so every test runs",
            );
            Ok(Some(Vec::new()))
        }
        Selection::Packages(names) if names.is_empty() => {
            ctx.ui().note(
                "nothing changed since the trunk reaches a package's tests, so none ran; `fun test --all` runs every test",
            );
            Ok(None)
        }
        Selection::Packages(names) => {
            ctx.ui().note(format!(
                "running the tests of the {} package(s) the change reaches: {}; `fun test --all` runs every test",
                names.len(),
                names.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            Ok(Some(
                names
                    .iter()
                    .map(|name| format!("package(={name})"))
                    .collect(),
            ))
        }
    }
}

/// The JUnit report under the target directory cargo resolves.
fn report_path(root: &Path) -> CliResult<PathBuf> {
    let metadata = metadata(root, &["--no-deps"])?;
    let target = metadata["target_directory"]
        .as_str()
        .ok_or_else(|| did_not_run("cargo metadata names no target directory".to_owned()))?;
    Ok(Path::new(target)
        .join("nextest")
        .join(PROFILE)
        .join("junit.xml"))
}

pub(crate) fn metadata(root: &Path, extra: &[&str]) -> CliResult<serde_json::Value> {
    let done = Command::new("cargo")
        .current_dir(root)
        .args(["metadata", "--format-version", "1"])
        .args(extra)
        .output()
        .map_err(|error| did_not_run(format!("cargo metadata cannot be run: {error}")))?;
    if !done.status.success() {
        return Err(did_not_run(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        )));
    }
    serde_json::from_slice(&done.stdout)
        .map_err(|error| did_not_run(format!("cargo metadata is unreadable: {error}")))
}

fn did_not_run(reason: String) -> CliError {
    CliError::TestsDidNotRun { reason }
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, write};

    use super::*;

    fn test_with(
        root: &Path,
        filters: &[String],
        all: bool,
        ctx: &CommandContext,
    ) -> CliResult<()> {
        let args = TestArgs {
            filters: filters.to_vec(),
            all,
            locked: false,
        };
        test(root, &args, ctx)
    }

    fn crate_with(profile: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a temp dir");
        write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .expect("a manifest");
        create_dir_all(root.path().join("src")).expect("src/");
        write(
            root.path().join("src/lib.rs"),
            "#[test]\nfn passes() {}\n\n#[test]\nfn fails() {\n    assert_eq!(1 + 1, 3, \"arithmetic broke\");\n}\n",
        )
        .expect("a lib");
        create_dir_all(root.path().join(".config")).expect(".config/");
        write(root.path().join(".config/nextest.toml"), profile).expect("a config");
        root
    }

    const AGENT: &str =
        "[profile.agent]\nfail-fast = false\n\n[profile.agent.junit]\npath = \"junit.xml\"\n";

    #[test]
    fn a_failing_test_is_named_from_the_report_and_refuses() {
        let root = crate_with(AGENT);
        let why = test_with(root.path(), &[], true, &CommandContext::new(true))
            .expect_err("one test fails");
        let CliError::TestsFailed { failed, report } = why else {
            panic!("not a failed run: {why:?}");
        };
        assert_eq!(failed, 1);
        let said = fs::read_to_string(&report).expect("the report");
        assert!(said.contains("arithmetic broke"), "{said}");
    }

    #[test]
    fn a_filter_narrows_the_run_to_what_passes() {
        let root = crate_with(AGENT);
        test_with(
            root.path(),
            &["test(=passes)".to_owned()],
            false,
            &CommandContext::new(true),
        )
        .expect("the passing test alone passes");
    }

    fn committed(profile: &str) -> tempfile::TempDir {
        let root = crate_with(profile);
        write(root.path().join(".gitignore"), "target/\n").expect("an ignore");
        let locked = Command::new("cargo")
            .current_dir(root.path())
            .args(["generate-lockfile", "--offline"])
            .status()
            .expect("cargo runs");
        assert!(locked.success(), "a lockfile");
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["config", "commit.gpgsign", "false"],
            vec!["add", "."],
            vec!["commit", "-m", "first"],
        ] {
            crate::helpers::git(root.path(), &args).expect("git sets up");
        }
        root
    }

    #[test]
    fn with_nothing_changed_since_the_trunk_no_test_runs() {
        let root = committed(AGENT);
        test_with(root.path(), &[], false, &CommandContext::new(true))
            .expect("nothing changed, so the failing test is not run");
        assert!(
            !root.path().join("target/nextest").exists(),
            "nextest never ran"
        );
    }

    #[test]
    fn a_change_to_a_package_runs_its_tests() {
        let root = committed(AGENT);
        write(
            root.path().join("src/lib.rs"),
            "#[test]\nfn passes() {}\n\n#[test]\nfn fails() {\n    assert_eq!(2, 3, \"still broke\");\n}\n",
        )
        .expect("an edit");
        let why = test_with(root.path(), &[], false, &CommandContext::new(true))
            .expect_err("the changed package's failing test runs");
        assert!(
            matches!(why, CliError::TestsFailed { failed: 1, .. }),
            "{why:?}"
        );
    }

    #[test]
    fn a_profile_without_a_report_has_no_verdict() {
        let root = crate_with("[profile.agent]\nfail-fast = false\n");
        let why = test_with(root.path(), &[], true, &CommandContext::new(true))
            .expect_err("no report, no verdict");
        assert!(matches!(why, CliError::TestsDidNotRun { .. }), "{why:?}");
        assert!(why.to_string().contains("junit"), "{why}");
    }
}
