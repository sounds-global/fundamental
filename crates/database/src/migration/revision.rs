//! [`Revision`]: the project's repository at one commit, read through `git`.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::migration::{MigrationError, MigrationResult};
use tracing::{debug, instrument};

/// A git hook's variables, cleared so the working directory alone says which repository is read.
const GIT_VARIABLES: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
];

/// The repository the working directory is in, at the commit a revision names.
#[derive(Debug, Clone)]
pub struct Revision {
    root: PathBuf,
    given: String,
    commit: String,
}

impl Revision {
    /// The commit `base` names in the repository `here` is in.
    ///
    /// # Errors
    /// [`MigrationError::BaseUnreadable`] when `here` is in no repository, git cannot run, or
    /// `base` names no commit.
    #[expect(
        clippy::cognitive_complexity,
        reason = "straight-line; each tracing event's expansion is counted"
    )]
    #[instrument(err(Debug))]
    pub fn resolve(base: &str, here: &Path) -> MigrationResult<Self> {
        let unreadable = |detail: String| MigrationError::BaseUnreadable {
            base: base.to_owned(),
            detail,
        };
        if base.trim().is_empty() || base.starts_with('-') {
            return Err(unreadable(
                "name the base as a commit: `migrate verify <revision>`".to_owned(),
            ));
        }
        let root = git(here, &["rev-parse", "--show-toplevel"])
            .map(PathBuf::from)
            .map_err(|detail| unreadable(format!("no repository here: {detail}")))?;
        let commit = git(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{base}^{{commit}}"),
            ],
        )
        .map_err(|_| unreadable("it names no commit of this repository".to_owned()))?;
        debug!(base, commit, root = %root.display(), "base revision resolved");
        Ok(Self {
            root,
            given: base.to_owned(),
            commit,
        })
    }

    /// The commit, as a full hash.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }

    /// Each `<name>.up.sql` in `dir` as the commit holds it, by `<name>`; `None` when `dir` is
    /// outside the repository.
    ///
    /// # Errors
    /// [`MigrationError::BaseUnreadable`] when git cannot list or read a file.
    #[expect(
        clippy::cognitive_complexity,
        reason = "straight-line; each tracing event's expansion is counted"
    )]
    #[instrument(skip(self), fields(commit = %self.commit), err(Debug))]
    pub fn files(&self, dir: &Path) -> MigrationResult<Option<BTreeMap<String, String>>> {
        let unreadable = |detail: String| MigrationError::BaseUnreadable {
            base: self.given.clone(),
            detail,
        };
        let Some(relative) = self.relative(dir).map_err(unreadable)? else {
            debug!(dir = %dir.display(), "outside the repository: every file counts as at the base");
            return Ok(None);
        };
        let files = self.read_dir(&relative).map_err(unreadable)?;
        debug!(dir = %relative, files = files.len(), "files at the base read");
        Ok(Some(files))
    }

    /// The `schema.json` in `dir` as the commit holds it; `None` when it holds none there or `dir`
    /// is outside the repository.
    ///
    /// # Errors
    /// [`MigrationError::BaseUnreadable`] when git cannot list or read the file.
    #[expect(
        clippy::cognitive_complexity,
        reason = "straight-line; each tracing event's expansion is counted"
    )]
    #[instrument(skip(self), fields(commit = %self.commit), err(Debug))]
    pub fn snapshot(&self, dir: &Path) -> MigrationResult<Option<String>> {
        let unreadable = |detail: String| MigrationError::BaseUnreadable {
            base: self.given.clone(),
            detail,
        };
        let Some(relative) = self.relative(dir).map_err(unreadable)? else {
            return Ok(None);
        };
        let path = format!("{relative}/schema.json");
        let listed = git(
            &self.root,
            &["ls-tree", "--name-only", "--full-tree", &self.commit, &path],
        )
        .map_err(unreadable)?;
        if listed.is_empty() {
            return Ok(None);
        }
        git(
            &self.root,
            &["cat-file", "blob", &format!("{}:{path}", self.commit)],
        )
        .map(Some)
        .map_err(unreadable)
    }
}

impl Revision {
    /// `dir` as a path below the root, `/`-separated; `None` when it is outside the repository.
    fn relative(&self, dir: &Path) -> Result<Option<String>, String> {
        let dir = absolute(dir).map_err(|error| error.to_string())?;
        let Ok(relative) = dir.strip_prefix(&self.root) else {
            return Ok(None);
        };
        Ok(Some(
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
        ))
    }

    /// Each `<name>.up.sql` in `relative`, a directory below the root, as the commit holds it.
    fn read_dir(&self, relative: &str) -> Result<BTreeMap<String, String>, String> {
        let listed = git(
            &self.root,
            &[
                "ls-tree",
                "--name-only",
                "--full-tree",
                &self.commit,
                &format!("{relative}/"),
            ],
        )?;
        listed
            .lines()
            .filter_map(|path| {
                let name = path.rsplit('/').next()?.strip_suffix(".up.sql")?;
                Some((name, path))
            })
            .map(|(name, path)| {
                let text = git(
                    &self.root,
                    &["cat-file", "blob", &format!("{}:{path}", self.commit)],
                )?;
                Ok((name.to_owned(), text))
            })
            .collect()
    }
}

/// `dir` made absolute, resolved by the file system as far as it exists.
fn absolute(dir: &Path) -> std::io::Result<PathBuf> {
    let dir = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(dir)
    };
    let mut existing = dir.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = existing.canonicalize() {
            let mut whole = resolved;
            for part in rest.iter().rev() {
                whole.push(part);
            }
            return Ok(whole);
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => {
                return Ok(dir
                    .components()
                    .filter(|c| *c != Component::CurDir)
                    .collect());
            }
        }
    }
}

/// Git's output for `args`, run in `directory`, trimmed at the end.
fn git(directory: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.current_dir(directory).args(args);
    for variable in GIT_VARIABLES {
        command.env_remove(variable);
    }
    let output = command
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if args.first() != Some(&"cat-file") {
        text.truncate(text.trim_end().len());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use crate::migration::MigrationError;

    use super::Revision;

    /// A repository holding `apps/ops/migrations/sql/0001_a.up.sql`, committed.
    fn repository() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let sql = dir.path().join("apps/ops/migrations/sql");
        std::fs::create_dir_all(&sql).expect("dirs");
        std::fs::write(sql.join("0001_a.up.sql"), "CREATE TABLE a (id UUID);\n").expect("file");
        std::fs::write(sql.join("notes.md"), "not a migration\n").expect("file");
        std::fs::write(sql.with_file_name("schema.json"), "{\"tables\": {}}\n").expect("file");
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "first",
            ],
        ] {
            let status = Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .expect("git runs");
            assert!(status.success(), "git {args:?}");
        }
        dir
    }

    fn revision_in(dir: &Path, base: &str) -> Result<Revision, MigrationError> {
        Revision::resolve(base, dir)
    }

    #[test]
    fn the_files_of_a_directory_are_read_as_the_commit_holds_them() {
        let repository = repository();
        let sql = repository.path().join("apps/ops/migrations/sql");
        std::fs::write(sql.join("0001_a.up.sql"), "-- edited since\n").expect("edit");
        let revision = revision_in(repository.path(), "HEAD").expect("HEAD is a commit");
        let files = revision
            .files(&sql)
            .expect("read")
            .expect("in the repository");
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(files["0001_a"], "CREATE TABLE a (id UUID);\n");
    }

    #[test]
    fn a_directory_the_commit_lacks_holds_no_files() {
        let repository = repository();
        let revision = revision_in(repository.path(), "HEAD").expect("HEAD is a commit");
        let files = revision
            .files(&repository.path().join("domain/newsroom/migrations/sql"))
            .expect("read")
            .expect("in the repository");
        assert!(files.is_empty(), "{files:?}");
    }

    #[test]
    fn a_directory_outside_the_repository_cannot_say() {
        let repository = repository();
        let elsewhere = tempfile::TempDir::new().expect("temp dir");
        let revision = revision_in(repository.path(), "HEAD").expect("HEAD is a commit");
        assert_eq!(revision.files(elsewhere.path()).expect("read"), None);
    }

    #[test]
    fn a_revision_that_names_no_commit_is_unreadable() {
        let repository = repository();
        let error = revision_in(repository.path(), "no-such-branch").expect_err("no commit");
        assert!(
            matches!(error, MigrationError::BaseUnreadable { .. }),
            "{error:?}"
        );
        let error = revision_in(repository.path(), "--upload-pack=x")
            .expect_err("an option is no revision");
        assert!(
            matches!(error, MigrationError::BaseUnreadable { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn the_snapshot_of_a_directory_is_read_as_the_commit_holds_it() {
        let repository = repository();
        let migrations = repository.path().join("apps/ops/migrations");
        std::fs::write(migrations.join("schema.json"), "<<<<<<< conflicted\n").expect("edit");
        let revision = revision_in(repository.path(), "HEAD").expect("HEAD is a commit");
        assert_eq!(
            revision.snapshot(&migrations).expect("read").as_deref(),
            Some("{\"tables\": {}}\n")
        );
        assert_eq!(
            revision
                .snapshot(&repository.path().join("domain/newsroom/migrations"))
                .expect("read"),
            None,
            "a directory the commit lacks holds no snapshot"
        );
    }
}
