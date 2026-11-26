//! `fun migrate rebase [<upstream>]`: the branch's migrations written again on top of main's.

use std::path::Path;

use super::MigrateRebaseArgs;
use super::generate::{self, Writes};
use super::revision::commit_of;
use crate::helpers::git;
use crate::{CliError, CliResult, CommandContext};

/// The tip, not the merge base: mid-merge `HEAD` is still the fork point, lacking main's new file.
const UPSTREAM: &str = "origin/main";

/// Resolves the upstream to one commit, then runs the owners' migration tests to rebase on it.
///
/// # Errors
/// [`CliError::RebaseUpstreamUnresolved`] when it names no commit, and what [`generate::run`] fails with.
pub(crate) fn run(args: MigrateRebaseArgs, ctx: CommandContext) -> CliResult<()> {
    let here = std::env::current_dir().map_err(|error| CliError::Io {
        path: ".".to_owned(),
        reason: error.to_string(),
    })?;
    let commit = resolve_upstream(&here, args.upstream.as_deref())?;
    generate::run(args.package, &Writes::Rebase(commit), ctx)
}

/// The commit `upstream` (default `origin/main`) names in the repository `here` is in.
///
/// # Errors
/// [`CliError::RebaseUpstreamUnresolved`] when git cannot find one there.
fn resolve_upstream(here: &Path, upstream: Option<&str>) -> CliResult<String> {
    let named = upstream.unwrap_or(UPSTREAM);
    let root = git(here, &["rev-parse", "--show-toplevel"]).map_err(|_| {
        CliError::RebaseUpstreamUnresolved {
            upstream: named.to_owned(),
            reason: "this directory is in no git repository".to_owned(),
        }
    })?;
    commit_of(Path::new(&root), named).map_err(|reason| CliError::RebaseUpstreamUnresolved {
        upstream: named.to_owned(),
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use fundamental::core::Error as _;

    use super::resolve_upstream;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn repository() -> (tempfile::TempDir, String) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("README"), "p\n").expect("file");
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "first"]);
        let head = git(dir.path(), &["rev-parse", "HEAD"]);
        (dir, head)
    }

    #[test]
    fn the_upstream_is_origin_mains_tip_unless_one_is_named() {
        let (dir, head) = repository();
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        assert_eq!(
            resolve_upstream(dir.path(), None).expect("origin/main"),
            head
        );
        let sub = dir.path().join("domain");
        std::fs::create_dir_all(&sub).expect("dir");
        assert_eq!(
            resolve_upstream(&sub, Some("main")).expect("main, from below the root"),
            head
        );
    }

    #[test]
    fn an_upstream_that_names_no_commit_is_refused_with_what_to_do() {
        let (dir, _) = repository();
        for upstream in [
            Some("no-such-branch"),
            Some("--upload-pack=x"),
            Some(""),
            None,
        ] {
            let error = resolve_upstream(dir.path(), upstream).expect_err("no commit");
            assert_eq!(error.error_code(), "CLI_081", "{upstream:?}");
            assert!(error.to_string().contains("git fetch"), "{error}");
        }
    }
}
