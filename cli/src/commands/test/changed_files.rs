//! The files a checkout has changed since its trunk.

use std::path::{Path, PathBuf};

use crate::CliResult;
use crate::helpers::git;

/// Every file `toplevel` changed since its trunk (the owning checkout's head), untracked included.
pub(super) fn changed_since_trunk(toplevel: &Path) -> CliResult<Vec<PathBuf>> {
    let trunk = git(&main_checkout(toplevel), &["rev-parse", "HEAD"])?;
    let base = git(toplevel, &["merge-base", "HEAD", &trunk])?;
    changed_since(toplevel, &base)
}

/// The checkout that owns `toplevel`'s worktrees: `toplevel` itself unless it is a worktree.
fn main_checkout(toplevel: &Path) -> PathBuf {
    git(
        toplevel,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .map(PathBuf::from)
    .and_then(|common| common.parent().map(Path::to_path_buf))
    .unwrap_or_else(|| toplevel.to_path_buf())
}

fn changed_since(toplevel: &Path, base: &str) -> CliResult<Vec<PathBuf>> {
    let tracked = git(toplevel, &["diff", "--name-only", base, "--"])?;
    let untracked = git(toplevel, &["ls-files", "--others", "--exclude-standard"])?;
    Ok(tracked
        .lines()
        .chain(untracked.lines())
        .filter(|line| !line.is_empty())
        .map(|line| toplevel.join(line))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use tempfile::TempDir;

    use super::*;

    fn repository() -> TempDir {
        let root = TempDir::new().expect("a temp dir");
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            git(root.path(), &args).expect("git sets up");
        }
        write(root.path().join("kept.rs"), "fn kept() {}\n").expect("a file");
        write(root.path().join("edited.rs"), "fn edited() {}\n").expect("a file");
        git(root.path(), &["add", "."]).expect("git adds");
        git(root.path(), &["commit", "-m", "first"]).expect("git commits");
        root
    }

    #[test]
    fn the_trunks_own_checkout_changed_only_what_is_not_committed() {
        let root = repository();
        write(root.path().join("edited.rs"), "fn edited() { 1; }\n").expect("an edit");
        write(root.path().join("new.rs"), "fn new() {}\n").expect("a new file");
        let mut changed = changed_since_trunk(root.path()).expect("git answers");
        changed.sort();
        assert_eq!(
            changed,
            [root.path().join("edited.rs"), root.path().join("new.rs")]
        );
    }

    #[test]
    fn a_worktree_changed_everything_it_committed_since_the_trunk() {
        let root = repository();
        let other = root.path().join("other");
        git(root.path(), &["worktree", "add", "-b", "feature", "other"]).expect("a worktree");
        write(other.join("edited.rs"), "fn edited() { 2; }\n").expect("an edit");
        git(&other, &["commit", "-am", "feature work"]).expect("git commits");
        write(root.path().join("kept.rs"), "fn kept() { 3; }\n").expect("trunk work");
        git(root.path(), &["commit", "-am", "trunk work"]).expect("git commits");
        let changed = changed_since_trunk(&other).expect("git answers");
        assert_eq!(
            changed,
            [other.join("edited.rs")],
            "the trunk's own work is not the worktree's"
        );
    }
}
