//! [`Migration`] discovery: scan `migrations/<engine>/` for paired up/down files.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

use crate::build_error::BuildError;

/// One discovered migration: an engine, an `up` script, and an optional `down`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Migration {
    /// The migration stem, e.g. `0001_init`.
    pub name: String,
    /// The engine directory it was found under, e.g. `sql` or `cql`.
    pub engine: String,
    /// The `*.up.sql` / `*.up.cql` file (absolute).
    pub up: PathBuf,
    /// The matching `*.down.*` file, if one exists.
    pub down: Option<PathBuf>,
    /// The lexicographic ordering key (the file stem, e.g. `0001_init`).
    pub order_key: String,
}

/// Discover migrations under `app_dir/migrations/<engine>/`, sorted by engine then key.
///
/// A missing down is allowed; a missing `migrations/` directory yields an empty vector.
///
/// # Errors
/// Returns [`BuildError::Io`] if the `migrations/` tree exists but cannot be read.
#[instrument(skip_all, fields(app_dir = %app_dir.display()), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn discover_migrations(app_dir: &Path) -> Result<Vec<Migration>, BuildError> {
    let root = app_dir.join("migrations");
    if !root.is_dir() {
        debug!("no migrations directory; empty set");
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for engine_entry in sorted_dir(&root)? {
        let engine_dir = engine_entry.path();
        if !engine_dir.is_dir() {
            continue;
        }
        let engine = engine_entry.file_name().to_string_lossy().to_string();
        collect_engine(&engine_dir, &engine, &mut out)?;
    }

    out.sort_by(|a, b| {
        a.engine
            .cmp(&b.engine)
            .then_with(|| a.order_key.cmp(&b.order_key))
    });
    debug!(count = out.len(), "discovered migrations");
    Ok(out)
}

fn collect_engine(
    engine_dir: &Path,
    engine: &str,
    out: &mut Vec<Migration>,
) -> Result<(), BuildError> {
    for entry in sorted_dir(engine_dir)? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = up_stem(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let down = find_down(engine_dir, &stem)?;
        if down.is_none() {
            debug!(engine, name = %stem, "migration has no down script");
        }
        out.push(Migration {
            name: stem.clone(),
            engine: engine.to_string(),
            up: path,
            down,
            order_key: stem,
        });
    }
    Ok(())
}

fn up_stem(file_name: &str) -> Option<String> {
    for ext in [".up.sql", ".up.cql"] {
        if let Some(stem) = file_name.strip_suffix(ext) {
            return Some(stem.to_string());
        }
    }
    None
}

fn find_down(engine_dir: &Path, stem: &str) -> Result<Option<PathBuf>, BuildError> {
    for ext in ["sql", "cql"] {
        let candidate = engine_dir.join(format!("{stem}.down.{ext}"));
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn sorted_dir(dir: &Path) -> Result<Vec<fs::DirEntry>, BuildError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| BuildError::io(dir, e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(dir: &Path, rel: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "-- sql").unwrap();
    }

    #[test]
    fn missing_dir_is_empty() {
        let d = TempDir::new().unwrap();
        assert!(discover_migrations(d.path()).unwrap().is_empty());
    }

    #[test]
    fn pairs_up_and_down_and_orders() {
        let d = TempDir::new().unwrap();
        touch(d.path(), "migrations/sql/0002_users.up.sql");
        touch(d.path(), "migrations/sql/0002_users.down.sql");
        touch(d.path(), "migrations/sql/0001_init.up.sql");
        touch(d.path(), "migrations/sql/0001_init.down.sql");
        let ms = discover_migrations(d.path()).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].order_key, "0001_init");
        assert_eq!(ms[1].order_key, "0002_users");
        assert!(ms[0].down.is_some());
        assert_eq!(ms[0].engine, "sql");
    }

    /// The engine order is alphabetical: `cql` sorts before `sql`.
    #[test]
    fn multiple_engines_sorted_by_engine_then_key() {
        let d = TempDir::new().unwrap();
        touch(d.path(), "migrations/cql/0001_init.up.cql");
        touch(d.path(), "migrations/sql/0001_init.up.sql");
        let ms = discover_migrations(d.path()).unwrap();
        assert_eq!(ms[0].engine, "cql");
        assert_eq!(ms[1].engine, "sql");
        assert!(ms[0].up.to_string_lossy().ends_with(".up.cql"));
    }

    #[test]
    fn missing_down_is_allowed() {
        let d = TempDir::new().unwrap();
        touch(d.path(), "migrations/sql/0001_init.up.sql");
        let ms = discover_migrations(d.path()).unwrap();
        assert_eq!(ms.len(), 1);
        assert!(ms[0].down.is_none());
    }

    #[test]
    fn non_up_files_ignored() {
        let d = TempDir::new().unwrap();
        touch(d.path(), "migrations/sql/README.md");
        touch(d.path(), "migrations/sql/0001_init.down.sql");
        let ms = discover_migrations(d.path()).unwrap();
        assert!(ms.is_empty(), "a lone down with no up is not a migration");
    }
}
