//! [`git`]: git's answer in a directory, read from that directory's repository alone.

use std::path::Path;
use std::process::Command;

use crate::{CliError, CliResult};

/// Git's trimmed output for `args` in `directory`, with a hook's `GIT_DIR` and kin cleared.
///
/// # Errors
/// [`CliError::CommandFailed`] when git cannot run or exits non-zero.
pub(crate) fn git(directory: &Path, args: &[&str]) -> CliResult<String> {
    let mut command = Command::new("git");
    command.current_dir(directory).args(args);
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
    ] {
        command.env_remove(variable);
    }
    let output = command.output().map_err(|e| CliError::Io {
        path: "git".to_owned(),
        reason: e.to_string(),
    })?;
    if !output.status.success() {
        return Err(CliError::CommandFailed {
            code: output.status.code().unwrap_or(1),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
