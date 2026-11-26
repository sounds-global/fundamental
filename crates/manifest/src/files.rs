//! The files a project's settings are read from, read bounded.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::source_file::SourceFile;
use crate::{ManifestError, ManifestFinding};

/// The project's settings, and what development runs on.
pub(crate) const FILE: &str = "fun.toml";
/// One developer's machine's overrides of development, never committed.
pub(crate) const LOCAL: &str = "fun.local.toml";
/// The environment `fun.toml` is: it has no file of its own.
pub(crate) const DEVELOPMENT: &str = "development";
/// Larger than any settings file needs to be.
const LARGEST_FILE: u64 = 1024 * 1024;

/// The file that overrides `fun.toml` in `environment`: `fun.<environment>.toml`.
pub(crate) fn environment_file(environment: &str) -> String {
    format!("fun.{environment}.toml")
}

/// The files `environment` reads, each overriding the one before: `fun.toml` and the
/// environment's file in `source_dir`, and in development `fun.local.toml` in `root`.
///
/// # Errors
///
/// [`ManifestError::Io`] when a file exists and cannot be read.
pub(crate) fn read(
    root: &Path,
    source_dir: &Path,
    environment: &str,
) -> Result<(Vec<SourceFile>, Vec<ManifestFinding>), ManifestError> {
    let mut findings = Vec::new();
    let development = environment_file(DEVELOPMENT);
    if source_dir.join(&development).exists() || root.join(&development).exists() {
        findings.push(ManifestFinding::DevelopmentFile { file: development });
    }
    let mut overrides: Vec<(&Path, String)> = Vec::new();
    if environment == DEVELOPMENT {
        overrides.push((root, LOCAL.to_owned()));
    } else {
        overrides.push((source_dir, environment_file(environment)));
    }
    let mut sources = Vec::new();
    if let Some(text) = read_bounded(&source_dir.join(FILE), FILE, &mut findings)? {
        sources.push(SourceFile {
            file: PathBuf::from(FILE),
            text,
        });
    }
    for (dir, file) in overrides {
        if let Some(text) = read_bounded(&dir.join(&file), &file, &mut findings)? {
            sources.push(SourceFile {
                file: PathBuf::from(file),
                text,
            });
        }
    }
    Ok((sources, findings))
}

/// What `fun.toml` reads as now, for an editor checking it did not change under it.
///
/// # Errors
///
/// [`ManifestError::Io`] when it cannot be read, [`ManifestError::Refused`] when it is too
/// large.
#[cfg(feature = "edit")]
pub(crate) fn read_current(source_dir: &Path) -> Result<Option<String>, ManifestError> {
    let mut findings = Vec::new();
    let text = read_bounded(&source_dir.join(FILE), FILE, &mut findings)?;
    if findings.is_empty() {
        Ok(text)
    } else {
        Err(ManifestError::refused(findings))
    }
}

/// Every file and directory of the old layout in `root`, each with the name its settings move
/// to.
pub(crate) fn old_layout(root: &Path) -> Vec<ManifestFinding> {
    let mut found = Vec::new();
    if root.join("fundamental.toml").exists() {
        found.push(renamed("fundamental.toml", FILE));
    }
    if is_old_settings_folder(&root.join("fundamental")) {
        found.push(renamed("fundamental/", FILE));
    }
    let Ok(entries) = fs::read_dir(root.join("config")) else {
        return found;
    };
    let mut configs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter_map(|name| name.strip_suffix(".toml").map(str::to_owned))
        .collect();
    configs.sort();
    for environment in configs {
        let new = if environment == DEVELOPMENT {
            FILE.to_owned()
        } else {
            environment_file(&environment)
        };
        found.push(renamed(&format!("config/{environment}.toml"), &new));
    }
    found
}

/// Whether `folder` holds settings files, not a project or checkout that is named `fundamental`.
fn is_old_settings_folder(folder: &Path) -> bool {
    if !folder.is_dir() || folder.join("Cargo.toml").exists() || folder.join(".git").exists() {
        return false;
    }
    fs::read_dir(folder).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let path = entry.path();
            path.extension()
                .is_some_and(|extension| extension == "toml")
                || (path.is_dir() && entry.file_name() == "apps")
        })
    })
}

fn renamed(old: &str, new: &str) -> ManifestFinding {
    ManifestFinding::Renamed {
        old: old.to_owned(),
        new: new.to_owned(),
    }
}

/// The file at `path`, named `file` in findings, or `None` when there is none; a file past the
/// size bound is a finding and reads as none.
fn read_bounded(
    path: &Path,
    file: &str,
    findings: &mut Vec<ManifestFinding>,
) -> Result<Option<String>, ManifestError> {
    let opened = match File::open(path) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ManifestError::io(path, &error)),
    };
    let mut text = String::new();
    opened
        .take(LARGEST_FILE + 1)
        .read_to_string(&mut text)
        .map_err(|error| ManifestError::io(path, &error))?;
    if text.len() as u64 > LARGEST_FILE {
        findings.push(ManifestFinding::TooLarge {
            file: file.to_owned(),
            reason: format!("a settings file holds at most {LARGEST_FILE} bytes"),
        });
        return Ok(None);
    }
    Ok(Some(text))
}
