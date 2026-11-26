use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, instrument, warn};

use crate::{InstanceLabel, InstanceRole, LocalError};

/// The checkout a verb runs in: where it is, and which instance of its project it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    root: PathBuf,
    role: InstanceRole,
}

impl Checkout {
    /// The checkout at `root`, as `role`.
    #[must_use]
    pub const fn new(root: PathBuf, role: InstanceRole) -> Self {
        Self { root, role }
    }

    /// Which instance the checkout at `root` is.
    ///
    /// A `label` (`--label` or `FUN_LOCAL_LABEL`) makes it that lane; otherwise a linked git
    /// worktree is a lane labelled from its branch, and anything else is main.
    ///
    /// # Errors
    ///
    /// [`LocalError::LabelInvalid`] (`LOCAL_019`) for a label that is not a DNS label, or a linked
    /// worktree whose branch makes none.
    #[instrument(fields(root = %root.display()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn detect(root: &Path, label: Option<&str>) -> Result<Self, LocalError> {
        let root = root.canonicalize().unwrap_or_else(|error| {
            warn!(%error, "the checkout's path could not be made canonical; using it as given");
            root.to_path_buf()
        });
        if let Some(label) = label {
            let role = InstanceRole::Lane(InstanceLabel::parse(label)?);
            return Ok(Self::new(root, role));
        }
        let role = match git(&root, &["rev-parse", "--git-dir", "--git-common-dir"]) {
            Some(dirs) => {
                let mut lines = dirs.lines().map(|line| root.join(line.trim()));
                let (git_dir, common) = (lines.next(), lines.next());
                let linked = match (git_dir, common) {
                    (Some(git_dir), Some(common)) => {
                        git_dir.canonicalize().ok() != common.canonicalize().ok()
                    }
                    _ => false,
                };
                if linked {
                    let branch = git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                        .ok_or_else(|| LocalError::LabelInvalid {
                            label: "a detached HEAD".to_owned(),
                        })?;
                    InstanceRole::Lane(InstanceLabel::from_branch(branch.trim())?)
                } else {
                    InstanceRole::Main
                }
            }
            None => InstanceRole::Main,
        };
        debug!(role = %role, "checkout detected");
        Ok(Self::new(root, role))
    }

    /// The checkout's root: the directory holding its `fun.toml`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Main or a lane.
    #[must_use]
    pub const fn role(&self) -> &InstanceRole {
        &self.role
    }
}

/// `git -C <root> <args>`'s output, or `None` when git is absent or the root is no repository.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout).ok(),
        Ok(output) => {
            debug!(?args, status = %output.status, "git answered no");
            None
        }
        Err(error) => {
            debug!(%error, "git could not be run; the checkout is taken as main");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use fundamental_core::Error as _;

    use super::Checkout;
    use crate::InstanceRole;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_linked_worktree_is_a_lane_labelled_from_its_branch() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let main = dir.path().join("main");
        std::fs::create_dir(&main).expect("a directory");
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "x"]);
        git(
            &main,
            &["worktree", "add", "-q", "-b", "Fix-Login", "../lane"],
        );
        let lane = Checkout::detect(&dir.path().join("lane"), None).expect("a lane");
        assert_eq!(
            lane.role().label().map(|label| label.as_str()),
            Some("fix-login")
        );
        let main = Checkout::detect(&main, None).expect("main");
        assert_eq!(main.role(), &InstanceRole::Main);
        let ci = Checkout::detect(&dir.path().join("main"), Some("ci")).expect("a named lane");
        assert_eq!(ci.role().label().map(|label| label.as_str()), Some("ci"));
        let refused = Checkout::detect(dir.path(), Some("CI")).map_err(|error| error.error_code());
        assert_eq!(refused, Err("LOCAL_019"));
    }
}
