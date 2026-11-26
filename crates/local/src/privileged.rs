use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tracing::{info, instrument};

use crate::LocalError;
use crate::tool::run;

/// The one way `fun local` acts as root: `sudo -- <tool> <fixed arguments>`, contents on
/// stdin, for a tool at a fixed root-owned path; never `PATH`, a shell, or a project's value.
#[derive(Debug, Clone, Copy)]
pub struct Privileged;

/// Where `sudo` itself is.
const SUDO: &[&str] = &["/usr/bin/sudo", "/bin/sudo"];

impl Privileged {
    /// The first of `candidates` that exists and that root alone can change.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when none exists, or the one that does is not root's own.
    pub fn tool(candidates: &[&str]) -> Result<PathBuf, LocalError> {
        let found = candidates
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .ok_or_else(|| LocalError::ToolFailed {
                argv: candidates.join(" | "),
                status: "not found".to_owned(),
                stderr: "none of these paths exists".to_owned(),
            })?;
        Self::root_owned(found)?;
        Ok(found.to_path_buf())
    }

    /// Refuses `path` unless it, after every link, and its directory belong to root and are writable by nobody else.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) naming what is wrong.
    pub fn root_owned(path: &Path) -> Result<(), LocalError> {
        let refuse = |why: String| LocalError::ToolFailed {
            argv: path.display().to_string(),
            status: "not run".to_owned(),
            stderr: why,
        };
        let real = fs::canonicalize(path).map_err(|error| refuse(error.to_string()))?;
        let parent = real.parent().unwrap_or(Path::new("/"));
        for checked in [real.as_path(), parent] {
            let metadata = fs::metadata(checked).map_err(|error| refuse(error.to_string()))?;
            if metadata.uid() != 0 {
                return Err(refuse(format!(
                    "{} is not owned by root",
                    checked.display()
                )));
            }
            if metadata.permissions().mode() & 0o022 != 0 {
                return Err(refuse(format!(
                    "{} is writable by its group or by anybody",
                    checked.display()
                )));
            }
        }
        Ok(())
    }

    /// Runs `tool` with `args` as root through `sudo`, writing `stdin` to it.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when `sudo` or `tool` is not root's own, or the run fails.
    #[instrument(skip(stdin), fields(tool = %tool.display()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn run(tool: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<String, LocalError> {
        let sudo = Self::tool(SUDO)?;
        Self::root_owned(tool)?;
        let mut argv: Vec<String> = vec!["--".to_owned(), tool.display().to_string()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        info!(argv = %argv.join(" "), "local.privileged");
        run(&sudo.display().to_string(), &argv, stdin)
    }

    /// Writes `contents` to `path` as root atomically: `tee` to a `0644` temporary beside it, then rename.
    ///
    /// # Errors
    ///
    /// As [`run`](Self::run).
    pub fn write(path: &Path, contents: &str) -> Result<(), LocalError> {
        let tee = Self::tool(&["/usr/bin/tee", "/bin/tee"])?;
        let chmod = Self::tool(&["/usr/bin/chmod", "/bin/chmod"])?;
        let mv = Self::tool(&["/usr/bin/mv", "/bin/mv"])?;
        let temporary = format!("{}.fundamental-new", path.display());
        Self::run(&tee, &[&temporary], Some(contents.as_bytes()))?;
        Self::run(&chmod, &["0644", &temporary], None)?;
        Self::run(&mv, &["-f", &temporary, &path.display().to_string()], None)?;
        Ok(())
    }

    /// Refuses to run as root: `fun local` acts as the developer and asks `sudo` per step.
    ///
    /// # Errors
    ///
    /// [`LocalError::RunAsRoot`] (`LOCAL_017`) when `euid` is root's.
    pub const fn refuse_root(euid: u32) -> Result<(), LocalError> {
        if euid == 0 {
            Err(LocalError::RunAsRoot)
        } else {
            Ok(())
        }
    }

    /// The effective uid of this process.
    #[must_use]
    pub fn effective_uid() -> u32 {
        rustix::process::geteuid().as_raw()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use fundamental_core::Error as _;

    use super::Privileged;

    #[test]
    fn fun_local_under_sudo_is_refused() {
        assert_eq!(
            Privileged::refuse_root(0).map_err(|error| error.error_code()),
            Err("LOCAL_017")
        );
        assert!(Privileged::refuse_root(1000).is_ok());
    }

    #[test]
    fn a_tool_outside_a_root_owned_path_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let planted = dir.path().join("tee");
        std::fs::write(&planted, "#!/bin/sh\n").expect("a planted tool");
        std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        let path = planted.display().to_string();
        let refused =
            Privileged::tool(&[&path]).map_err(|error| (error.error_code(), error.to_string()));
        let (code, message) = refused.expect_err("a tool the developer owns");
        assert_eq!(code, "LOCAL_012");
        assert!(message.contains("not owned by root"), "{message}");
        let missing =
            Privileged::tool(&["/nonexistent/fundamental/tee"]).map_err(|error| error.error_code());
        assert_eq!(missing, Err("LOCAL_012"));
    }

    #[test]
    fn a_root_owned_system_tool_is_accepted() {
        let found = Privileged::tool(&["/usr/bin/tee", "/bin/tee"]).expect("tee is root's own");
        assert!(found.is_absolute());
    }
}
