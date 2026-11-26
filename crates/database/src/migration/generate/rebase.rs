//! An owner's branch migrations written again on top of main's, as `fun migrate rebase` asks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::Snapshot;
use super::next_statements;
use super::owner::{
    GENERATED_HEADER, GenerateError, generated_text, lints_of, migrations_dir, models,
};
use crate::migration::{EmbeddedMigrations, Revision, content_hash};

/// An owner's migrations as main holds them; empty when main holds none of this owner's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Main {
    /// Each file, `<number>_<name>` to its statements.
    pub files: BTreeMap<String, String>,
    /// Its `migrations/schema.json`.
    pub snapshot: String,
}

impl Main {
    /// `set`'s migrations as the commit `revision` holds them, read through git.
    ///
    /// # Errors
    /// [`GenerateError::NotGenerated`] for a set not generated from its models, and
    /// [`GenerateError::MainUnreadable`] when main's files cannot be read.
    pub fn read(revision: &Revision, set: &EmbeddedMigrations) -> Result<Self, GenerateError> {
        let owner = set.owner();
        let unreadable = |detail: String| GenerateError::MainUnreadable { owner, detail };
        let dir = migrations_dir(set)?;
        let files = revision
            .files(&dir.join("sql"))
            .map_err(|error| unreadable(error.to_string()))?;
        let snapshot = revision
            .snapshot(&dir)
            .map_err(|error| unreadable(error.to_string()))?;
        match (files, snapshot) {
            (Some(files), Some(snapshot)) => Ok(Self { files, snapshot }),
            (Some(files), None) if files.is_empty() => Ok(Self::default()),
            (None, _) => Err(unreadable(format!(
                "{} is outside the repository",
                dir.display()
            ))),
            (Some(_), None) => Err(unreadable(format!(
                "{} holds no migrations/schema.json at {}",
                owner,
                revision.commit()
            ))),
        }
    }
}

/// What [`rebase`] did: the branch files it removed, and the one it wrote in their place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rebased {
    /// The branch's files removed, in order.
    pub removed: Vec<PathBuf>,
    /// The file written after main's last; none when main already makes what the models declare.
    pub written: Option<PathBuf>,
}

/// Writes `set`'s branch migrations again on top of `main`: the branch's files are removed and
/// the models' difference from main's snapshot written as one file after main's last.
///
/// # Errors
/// [`GenerateError::MainUnreadable`], [`GenerateError::MainFileMissing`],
/// [`GenerateError::MainFileChanged`] or [`GenerateError::WrittenByHand`] when main or the tree
/// is not as it must be; a refusal, or a file that could not be written.
pub fn rebase(set: &EmbeddedMigrations, main: &Main) -> Result<Rebased, GenerateError> {
    if *main == Main::default() {
        return Ok(Rebased::default());
    }
    let owner = set.owner();
    let before = main_snapshot(set, main)?;
    let branch = branch_files(set, main)?;
    let statements = next_statements(&before, &models(set, None), &lints_of(set)?)
        .map_err(|undeclared| GenerateError::Refused { owner, undeclared })?;
    let dir = migrations_dir(set)?;
    let sql = dir.join("sql");
    let written = match branch.last() {
        Some(newest) if !statements.is_empty() => Some(sql.join(format!(
            "{:04}_{}.up.sql",
            main.files.len() + 1,
            without_number(newest)
        ))),
        _ => None,
    };
    let mut removed = Vec::new();
    for name in &branch {
        let file = sql.join(format!("{name}.up.sql"));
        remove(&file)?;
        if written.as_ref() != Some(&file) {
            removed.push(file);
        }
    }
    let after = match &written {
        Some(file) => {
            let text = generated_text(&statements);
            std::fs::create_dir_all(&sql).map_err(GenerateError::Io)?;
            std::fs::write(file, &text).map_err(GenerateError::Io)?;
            models(set, Some(content_hash(&text)))
        }
        None => before,
    };
    std::fs::write(dir.join("schema.json"), after.to_file()).map_err(GenerateError::Io)?;
    Ok(Rebased { removed, written })
}

/// The files of `set` that main lacks, in order, once the tree is checked against main.
fn branch_files<'a>(
    set: &'a EmbeddedMigrations,
    main: &Main,
) -> Result<Vec<&'a str>, GenerateError> {
    let owner = set.owner();
    let ours: BTreeMap<&str, &str> = set
        .migrations()
        .iter()
        .map(|migration| (migration.name(), migration.up()))
        .collect();
    for (name, up) in &main.files {
        match ours.get(name.as_str()) {
            None => {
                return Err(GenerateError::MainFileMissing {
                    owner,
                    migration: name.clone(),
                });
            }
            Some(ours) if ours != up => {
                return Err(GenerateError::MainFileChanged {
                    owner,
                    migration: name.clone(),
                });
            }
            Some(_) => {}
        }
    }
    let branch: Vec<(&str, &str)> = ours
        .into_iter()
        .filter(|(name, _)| !main.files.contains_key(*name))
        .collect();
    if let Some((name, _)) = branch
        .iter()
        .find(|(_, up)| !up.starts_with(GENERATED_HEADER))
    {
        return Err(GenerateError::WrittenByHand {
            owner,
            migration: (*name).to_owned(),
        });
    }
    Ok(branch.into_iter().map(|(name, _)| name).collect())
}

fn main_snapshot(set: &EmbeddedMigrations, main: &Main) -> Result<Snapshot, GenerateError> {
    let owner = set.owner();
    let snapshot =
        Snapshot::from_file(&main.snapshot).map_err(|error| GenerateError::MainUnreadable {
            owner,
            detail: format!("its migrations/schema.json is not a snapshot: {error}"),
        })?;
    let last = main.files.values().last().map(|up| content_hash(up));
    if snapshot.parent != last {
        return Err(GenerateError::MainUnreadable {
            owner,
            detail: "its migrations/schema.json is not the schema after its last migration"
                .to_owned(),
        });
    }
    Ok(snapshot)
}

fn without_number(name: &str) -> &str {
    name.split_once('_')
        .filter(|(number, _)| number.chars().all(|c| c.is_ascii_digit()))
        .map_or(name, |(_, rest)| rest)
}

fn remove(file: &Path) -> Result<(), GenerateError> {
    match std::fs::remove_file(file) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(GenerateError::Io(error)),
        _ => Ok(()),
    }
}
