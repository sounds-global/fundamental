//! `fun migrate verify`: every app's migrations replayed from scratch and as the upgrade from the base.

use std::path::Path;

use tracing::{info, instrument};

use super::revision::commit_of;
use super::{MigrateVerifyArgs, spawn};
use crate::helpers::{Project, git};
use crate::{CliError, CliResult, CommandContext};

const MAIN: &str = "origin/main";

/// Runs every app's `migrate verify <sha>` against one resolved base, returning the first failure.
///
/// # Errors
/// [`CliError::VerifyBaseUnresolved`], [`CliError::NoMigrator`], or the first failure
/// ([`CliError::MigrateFailed`]).
#[instrument(skip(ctx), err(Debug))]
#[expect(
    clippy::cognitive_complexity,
    reason = "straight-line; each tracing event's expansion is counted"
)]
pub(crate) fn run(args: MigrateVerifyArgs, ctx: CommandContext) -> CliResult<()> {
    let project = Project::discover()?;
    let base = resolve_base(project.root(), args.base.as_deref())?;
    let apps = apps_to_verify(&project, args.app)?;
    info!(base, apps = apps.len(), "verifying migrations");
    let failures: Vec<CliError> = apps
        .iter()
        .filter_map(|app| spawn::run(Some(app), None, &["verify", &base], &ctx).err())
        .collect();
    failures.into_iter().next().map_or(Ok(()), Err)
}

/// The app named, or every app with a migration binary.
///
/// # Errors
/// [`CliError::NoMigrator`] when none is named and no app has one.
fn apps_to_verify(project: &Project, app: Option<String>) -> CliResult<Vec<String>> {
    if let Some(app) = app {
        return Ok(vec![app]);
    }
    let apps = project.apps_with_migrator();
    if apps.is_empty() {
        return Err(CliError::NoMigrator);
    }
    Ok(apps)
}

/// The commit `base` names, or the merge base of `HEAD` and `origin/main`.
///
/// # Errors
/// [`CliError::VerifyBaseUnresolved`] when it names no commit of the repository at `root`.
fn resolve_base(root: &Path, base: Option<&str>) -> CliResult<String> {
    let unresolved = |base: &str, reason: &str| CliError::VerifyBaseUnresolved {
        base: base.to_owned(),
        reason: reason.to_owned(),
    };
    let named = match base {
        Some(base) => base.to_owned(),
        None => git(root, &["merge-base", "HEAD", MAIN]).map_err(|_| {
            unresolved(
                &format!("merge-base(HEAD, {MAIN})"),
                "this branch shares no commit with origin/main here; fetch it, or name one",
            )
        })?,
    };
    commit_of(root, &named).map_err(|reason| unresolved(&named, reason))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use fundamental::core::Error as _;

    use super::{apps_to_verify, resolve_base};
    use crate::helpers::Project;

    fn repository() -> (tempfile::TempDir, String) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("fun.toml"), "[project]\nname = \"p\"\n").expect("file");
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["add", "."],
            &["commit", "-q", "-m", "first"],
        ] {
            let status = Command::new("git")
                .current_dir(dir.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .status()
                .expect("git runs");
            assert!(status.success(), "git {args:?}");
        }
        let head = Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git runs");
        let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
        (dir, head)
    }

    #[test]
    fn a_named_base_is_resolved_to_its_commit_before_any_binary_runs() {
        let (dir, head) = repository();
        assert_eq!(resolve_base(dir.path(), Some("main")).expect("main"), head);
    }

    #[test]
    fn a_base_that_names_no_commit_is_refused_with_its_code() {
        let (dir, _) = repository();
        for base in ["no-such-branch", "--upload-pack=x", ""] {
            let error = resolve_base(dir.path(), Some(base)).expect_err(base);
            assert_eq!(error.error_code(), "CLI_079", "{base:?}");
        }
    }

    #[test]
    fn with_no_app_named_every_app_with_a_migrator_is_verified() {
        let (dir, _) = repository();
        for (app, bins) in [("ops", "[[bin]]\nname = \"ops-migrate\"\n"), ("www", "")] {
            let app_dir = dir.path().join("apps").join(app);
            std::fs::create_dir_all(&app_dir).expect("app dir");
            std::fs::write(app_dir.join("Cargo.toml"), bins).expect("manifest");
        }
        let project = Project::discover_from(dir.path()).expect("a project");
        assert_eq!(apps_to_verify(&project, None).expect("ops"), ["ops"]);
        assert_eq!(
            apps_to_verify(&project, Some("www".to_owned())).expect("named"),
            ["www"]
        );
    }

    #[test]
    fn with_no_migrator_there_is_nothing_to_verify() {
        let (dir, _) = repository();
        let project = Project::discover_from(dir.path()).expect("a project");
        let error = apps_to_verify(&project, None).expect_err("no app");
        assert_eq!(error.error_code(), "CLI_080");
    }

    #[test]
    fn with_no_origin_main_the_default_base_says_what_to_do() {
        let (dir, _) = repository();
        let error = resolve_base(dir.path(), None).expect_err("no origin here");
        assert_eq!(error.error_code(), "CLI_079");
        assert!(error.to_string().contains("--base"), "{error}");
    }
}
