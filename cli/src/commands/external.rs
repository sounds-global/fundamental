//! A command `fun` does not have: `fun <name> …` runs the program `fun-<name>` from `PATH` with
//! the words after the name, as `cargo` runs `cargo-<name>`, so a tool can add a command to `fun`
//! without being part of it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{CliError, CliResult};

/// Runs `fun-<name>` for `words`, `name` first, and exits as it exits.
///
/// # Errors
/// [`CliError::NoSuchCommand`] when no `fun-<name>` is on `PATH`, [`CliError::Io`] when it
/// cannot be started.
pub(crate) fn run(words: Vec<OsString>) -> CliResult<()> {
    let Some((name, rest)) = words.split_first() else {
        return Err(CliError::NoSuchCommand {
            name: String::new(),
        });
    };
    let name = name.to_string_lossy().into_owned();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(program) = find(&name, &path) else {
        return Err(CliError::NoSuchCommand { name });
    };
    let status = Command::new(&program)
        .args(rest)
        .status()
        .map_err(|e| CliError::Io {
            path: program.display().to_string(),
            reason: e.to_string(),
        })?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1))
    }
}

/// The program `fun-<name>` in the first directory of `path` that holds one, if any.
pub(crate) fn find(name: &str, path: &OsStr) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\']) {
        return None;
    }
    let program = format!("fun-{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .map(|dir| dir.join(&program))
        .find(|candidate| is_program(candidate))
}

#[cfg(unix)]
fn is_program(candidate: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    candidate
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_program(candidate: &Path) -> bool {
    candidate.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn program(dir: &Path, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("a program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("runnable");
    }

    #[cfg(unix)]
    #[test]
    fn a_command_fun_lacks_is_the_first_fun_program_of_that_name_on_the_path() {
        let first = tempfile::tempdir().expect("a directory");
        let second = tempfile::tempdir().expect("a directory");
        program(second.path(), "fun-deploy");
        program(first.path(), "fun-other");
        std::fs::write(first.path().join("fun-deploy"), "not runnable").expect("a file");
        let path = std::env::join_paths([first.path(), second.path()]).expect("a path");
        assert_eq!(
            find("deploy", &path),
            Some(second.path().join("fun-deploy")),
            "a file that cannot run is passed over"
        );
        assert_eq!(find("absent", &path), None);
        assert_eq!(find("../deploy", &path), None, "a name is never a path");
    }

    #[test]
    fn an_unknown_command_is_parsed_whole_for_its_program() {
        use clap::Parser as _;
        let cli = crate::Cli::try_parse_from(["fun", "deploy", "list", "--all"]).expect("parsed");
        let crate::commands::Command::External(words) = cli.command else {
            panic!("an unknown command is external");
        };
        assert_eq!(words, ["deploy", "list", "--all"]);
    }
}
