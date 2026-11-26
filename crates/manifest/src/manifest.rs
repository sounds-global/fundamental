//! A project's settings: `fun.toml`, and the file that overrides it in one environment.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use toml::Value;
use tracing::{debug, instrument};

use crate::files::{self, DEVELOPMENT, FILE};
use crate::leaf::Leaf;
use crate::parsed_file::parse_leaves;
use crate::source_file::SourceFile;
use crate::{KeyPath, ManifestError, ManifestFinding, Origin};

/// A project's settings in one environment, read and checked: `fun.toml`, and over it the file
/// that overrides it there. A table an override names keeps the keys it does not name; a list
/// it names is replaced.
#[derive(Debug, Clone)]
pub struct Manifest {
    root: PathBuf,
    source_dir: PathBuf,
    environment: String,
    files: Vec<PathBuf>,
    merged: toml::Table,
    origins: BTreeMap<KeyPath, Origin>,
    digest: String,
}

impl Manifest {
    /// The settings file every project has: `fun.toml`.
    pub const FILE: &'static str = FILE;

    /// Reads the settings of the project at `root` (empty: the working directory) as
    /// development runs on them: `fun.toml`, and `fun.local.toml` over it.
    ///
    /// # Errors
    ///
    /// [`ManifestError::NotFound`] when there is no `fun.toml`, [`ManifestError::Io`] when a
    /// file cannot be read, and [`ManifestError::Refused`] with every finding when the files
    /// are not valid settings or the project still has the old layout.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, ManifestError> {
        Self::load_for(root, DEVELOPMENT)
    }

    /// Reads the settings of the project at `root` in `environment`: `fun.toml`, and
    /// `fun.<environment>.toml` over it when there is one.
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load).
    #[instrument(skip_all, fields(root = %root.as_ref().display(), environment))]
    pub fn load_for(root: impl AsRef<Path>, environment: &str) -> Result<Self, ManifestError> {
        let root = non_empty(root.as_ref());
        let mut old = files::old_layout(&root);
        let source_dir = match resolve(&root) {
            Err(ManifestError::NotFound { .. }) if !old.is_empty() => {
                return Err(ManifestError::refused(old));
            }
            resolved => resolved?.1,
        };
        let (sources, findings) = files::read(&root, &source_dir, environment)?;
        old.extend(findings);
        let findings = old;
        Self::build(&root, &source_dir, environment, &sources, findings)
    }

    /// Reads the settings of the nearest project at or above `start`, as development runs on
    /// them.
    ///
    /// # Errors
    ///
    /// [`ManifestError::NotFound`] when no directory at or above `start` holds a `fun.toml`;
    /// otherwise as [`load`](Self::load).
    pub fn discover(start: impl AsRef<Path>) -> Result<Self, ManifestError> {
        Self::load(Self::find_root(start)?)
    }

    /// The nearest directory at or above `start` holding a `fun.toml` (a dangling link counts,
    /// so the load refuses it rather than walking past it) or a project of the old layout.
    ///
    /// # Errors
    ///
    /// [`ManifestError::NotFound`] when no such directory exists, [`ManifestError::Io`] when
    /// `start` cannot be made absolute.
    pub fn find_root(start: impl AsRef<Path>) -> Result<PathBuf, ManifestError> {
        let start = non_empty(start.as_ref());
        let absolute =
            std::path::absolute(&start).map_err(|error| ManifestError::io(&start, &error))?;
        absolute
            .ancestors()
            .find(|directory| {
                [FILE, "fundamental.toml"].iter().any(|file| {
                    directory
                        .join(file)
                        .symlink_metadata()
                        .is_ok_and(|metadata| !metadata.is_dir())
                })
            })
            .map(Path::to_path_buf)
            .ok_or_else(|| ManifestError::NotFound {
                start: start.display().to_string(),
            })
    }

    /// The settings `sources` hold, the first the base and each after it an override.
    pub(crate) fn build(
        root: &Path,
        source_dir: &Path,
        environment: &str,
        sources: &[SourceFile],
        mut findings: Vec<ManifestFinding>,
    ) -> Result<Self, ManifestError> {
        let mut merged = toml::Table::new();
        let mut origins: BTreeMap<KeyPath, Origin> = BTreeMap::new();
        let leaves: Vec<Leaf> = sources
            .iter()
            .filter_map(|source| {
                parse_leaves(source)
                    .map_err(|finding| findings.push(finding))
                    .ok()
            })
            .flatten()
            .collect();
        for leaf in leaves {
            apply(&mut merged, &mut origins, leaf, &mut findings);
        }
        if !findings.is_empty() {
            return Err(ManifestError::refused(findings));
        }
        let manifest = Self {
            root: root.to_path_buf(),
            source_dir: source_dir.to_path_buf(),
            environment: environment.to_owned(),
            files: sources.iter().map(|source| source.file.clone()).collect(),
            merged,
            origins,
            digest: digest_of(sources),
        };
        debug!(
            source_dir = %manifest.source_dir.display(),
            environment,
            files = manifest.files.len(),
            digest = %manifest.digest,
            "settings loaded"
        );
        Ok(manifest)
    }

    /// The project directory as given: the base a relative `${file:…}` secret resolves from.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory `fun.toml` really lives in, and an environment's file beside it.
    #[must_use]
    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    /// The environment these settings are for.
    #[must_use]
    pub fn environment(&self) -> &str {
        &self.environment
    }

    /// Every file read, `fun.toml` first and then the file that overrides it.
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The settings as one table.
    #[must_use]
    pub fn as_table(&self) -> &toml::Table {
        &self.merged
    }

    /// The value at `key`, if any.
    #[must_use]
    pub fn get(&self, key: &KeyPath) -> Option<&Value> {
        let (first, rest) = key.segments().split_first()?;
        rest.iter()
            .try_fold(self.merged.get(first)?, |value, segment| value.get(segment))
    }

    /// The value at `key` read as `T`, or `None` when the settings have none.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Shape`] when the value is there and does not read as `T`, naming the
    /// files and lines it comes from.
    pub fn table<T: DeserializeOwned>(&self, key: &KeyPath) -> Result<Option<T>, ManifestError> {
        let Some(value) = self.get(key) else {
            return Ok(None);
        };
        value
            .clone()
            .try_into()
            .map(Some)
            .map_err(|error: toml::de::Error| ManifestError::Shape {
                key: key.to_string(),
                at: self
                    .origins(key)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                cause: error.message().to_owned(),
            })
    }

    /// The whole settings read as `T`.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Shape`] when the settings do not read as `T`.
    pub fn whole<T: DeserializeOwned>(&self) -> Result<T, ManifestError> {
        Value::Table(self.merged.clone())
            .try_into()
            .map_err(|error: toml::de::Error| ManifestError::Shape {
                key: "the whole settings".to_owned(),
                at: self
                    .files
                    .iter()
                    .map(|file| file.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                cause: error.message().to_owned(),
            })
    }

    /// Where `key` is written: one origin for a value, the first line in each file for a
    /// table spread over several. Empty when the settings have no such key.
    #[must_use]
    pub fn origins(&self, key: &KeyPath) -> Vec<Origin> {
        let mut found: Vec<Origin> = Vec::new();
        for (leaf, origin) in &self.origins {
            if (leaf.starts_with(key) || key.starts_with(leaf))
                && !found.iter().any(|seen| seen.file() == origin.file())
            {
                found.push(origin.clone());
            }
        }
        found
    }

    /// A BLAKE3 digest over every file's name and bytes, naming exactly these settings.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// The project directory as given (empty meaning the working directory) and the directory
/// its `fun.toml` really lives in.
pub(crate) fn resolve(root: &Path) -> Result<(PathBuf, PathBuf), ManifestError> {
    let root = non_empty(root);
    let named = root.join(FILE);
    let resolved = match named.canonicalize() {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ManifestError::NotFound {
                start: root.display().to_string(),
            });
        }
        Err(error) => return Err(ManifestError::io(&named, &error)),
    };
    let source_dir = resolved
        .parent()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
    Ok((root, source_dir))
}

fn non_empty(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

/// Writes `leaf` into `merged`; a value where a table is, or the reverse, is a finding.
fn apply(
    merged: &mut toml::Table,
    origins: &mut BTreeMap<KeyPath, Origin>,
    leaf: Leaf,
    findings: &mut Vec<ManifestFinding>,
) {
    if put(merged, leaf.key.segments(), &leaf.value) {
        origins.retain(|key, _| !(key.starts_with(&leaf.key) && key != &leaf.key));
        origins.insert(leaf.key, leaf.origin);
    } else {
        let base = origins
            .iter()
            .find(|(other, _)| other.starts_with(&leaf.key) || leaf.key.starts_with(other))
            .map_or_else(|| FILE.to_owned(), |(_, origin)| origin.to_string());
        findings.push(ManifestFinding::Override {
            at: leaf.origin.to_string(),
            key: leaf.key.to_string(),
            base,
        });
    }
}

/// Puts `value` at `segments` in `table`, replacing a value already there; `false` when a
/// table stands where a value goes, or a value where a table goes.
fn put(table: &mut toml::Table, segments: &[String], value: &Value) -> bool {
    let Some((first, rest)) = segments.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return match (table.get_mut(first), value) {
            (None, _) => {
                table.insert(first.clone(), value.clone());
                true
            }
            (Some(Value::Table(_)), Value::Table(new)) => new.is_empty(),
            (Some(Value::Table(existing)), _) => {
                existing.is_empty() && {
                    table.insert(first.clone(), value.clone());
                    true
                }
            }
            (Some(_), Value::Table(_)) => false,
            (Some(existing), _) => {
                *existing = value.clone();
                true
            }
        };
    }
    match table
        .entry(first.clone())
        .or_insert_with(|| Value::Table(toml::Table::new()))
    {
        Value::Table(next) => put(next, rest, value),
        _ => false,
    }
}

fn digest_of(sources: &[SourceFile]) -> String {
    let mut hasher = blake3::Hasher::new_derive_key("fundamental settings 2026-11-25 files");
    for source in sources {
        let file = source.file.to_string_lossy();
        hasher.update(&(file.len() as u64).to_le_bytes());
        hasher.update(file.as_bytes());
        hasher.update(&(source.text.len() as u64).to_le_bytes());
        hasher.update(source.text.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
