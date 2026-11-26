//! A revision a `fun migrate` command is given, resolved to one commit before anything runs.

use std::path::Path;

use crate::helpers::git;

/// The commit `named` names at `root`; an option-like or empty revision is refused before git reads it.
pub(super) fn commit_of(root: &Path, named: &str) -> Result<String, &'static str> {
    if named.starts_with('-') || named.trim().is_empty() {
        return Err("a revision is not an option or empty");
    }
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{named}^{{commit}}"),
        ],
    )
    .map_err(|_| "git does not know it here")
}
