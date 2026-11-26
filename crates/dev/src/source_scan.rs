//! [`SourceScan`]: a snapshot of every watched source file, for change detection.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tracing::{debug, instrument};

use crate::dev_error::DevError;

/// Written by the build, so walking them would make every build trigger the next.
const SKIPPED_DIRS: &[&str] = &[
    "target",
    "dist",
    ".git",
    "node_modules",
    "pkg",
    "gen",
    "platforms",
    ".venv",
    "__pycache__",
];

/// A snapshot of the watched tree: every file's length and modification time.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceScan {
    entries: BTreeMap<PathBuf, Stamp>,
}

/// Length beside mtime catches a rewrite within one coarse timestamp tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
}

impl SourceScan {
    /// Walk `roots` and record every file beneath them.
    ///
    /// # Errors
    /// [`DevError::UnwatchableRoot`] if a root cannot be read.
    #[instrument(skip_all, fields(roots = roots.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn take(roots: &[PathBuf]) -> Result<Self, DevError> {
        let mut entries = BTreeMap::new();
        for root in roots {
            let meta = fs::metadata(root).map_err(|e| DevError::UnwatchableRoot {
                root: root.clone(),
                reason: e.to_string(),
            })?;
            if meta.is_file() {
                insert(&mut entries, root, &meta);
            } else {
                walk(root, &mut entries)?;
            }
        }
        debug!(files = entries.len(), "scanned the watched tree");
        Ok(Self { entries })
    }

    /// The number of files in the snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the snapshot is empty, always a sign the roots were wrong.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The paths added, removed, or restamped between `self` and `next`.
    #[must_use]
    pub fn changes(&self, next: &Self) -> Vec<PathBuf> {
        let mut changed: Vec<PathBuf> = next
            .entries
            .iter()
            .filter(|(path, stamp)| self.entries.get(*path) != Some(*stamp))
            .map(|(path, _)| path.clone())
            .collect();
        changed.extend(
            self.entries
                .keys()
                .filter(|path| !next.entries.contains_key(*path))
                .cloned(),
        );
        changed.sort();
        changed.dedup();
        changed
    }
}

fn walk(dir: &Path, entries: &mut BTreeMap<PathBuf, Stamp>) -> Result<(), DevError> {
    let unwatchable = |e: std::io::Error| DevError::UnwatchableRoot {
        root: dir.to_path_buf(),
        reason: e.to_string(),
    };

    for entry in fs::read_dir(dir).map_err(unwatchable)? {
        let entry = entry.map_err(unwatchable)?;
        let path = entry.path();
        // `file_type` does not follow symlinks, so a link into `target/` stays unwatched.
        let file_type = entry.file_type().map_err(unwatchable)?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIPPED_DIRS.contains(&name.as_ref()) || name.starts_with('.') {
                continue;
            }
            walk(&path, entries)?;
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            insert(entries, &path, &meta);
        }
    }
    Ok(())
}

/// A platform without mtime stamps at the epoch, so a length change still registers.
fn insert(entries: &mut BTreeMap<PathBuf, Stamp>, path: &Path, meta: &fs::Metadata) {
    entries.insert(
        path.to_path_buf(),
        Stamp {
            len: meta.len(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tree() -> TempDir {
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src/nested")).expect("src");
        fs::create_dir_all(tmp.path().join("target/debug")).expect("target");
        fs::create_dir_all(tmp.path().join("dist")).expect("dist");
        fs::write(tmp.path().join("src/a.rs"), "fn a() {}").expect("a");
        fs::write(tmp.path().join("src/nested/b.rs"), "fn b() {}").expect("b");
        fs::write(tmp.path().join("target/debug/out"), "binary").expect("out");
        fs::write(tmp.path().join("dist/app.js"), "//").expect("js");
        tmp
    }

    #[test]
    fn walks_nested_sources_and_skips_build_output() {
        let tmp = tree();
        let scan = SourceScan::take(&[tmp.path().to_path_buf()]).expect("scanned");
        assert_eq!(scan.len(), 2, "only the two sources: {scan:?}");
        assert!(!scan.is_empty());
    }

    #[test]
    fn an_unchanged_tree_reports_no_changes() {
        let tmp = tree();
        let before = SourceScan::take(&[tmp.path().to_path_buf()]).expect("first");
        let after = SourceScan::take(&[tmp.path().to_path_buf()]).expect("second");
        assert!(before.changes(&after).is_empty());
        assert_eq!(before, after);
    }

    #[test]
    fn an_edit_is_reported_even_within_one_timestamp_tick() {
        let tmp = tree();
        let before = SourceScan::take(&[tmp.path().to_path_buf()]).expect("first");
        fs::write(tmp.path().join("src/a.rs"), "fn a() { /* edited */ }").expect("edit");
        let after = SourceScan::take(&[tmp.path().to_path_buf()]).expect("second");

        let changes = before.changes(&after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(changes[0].ends_with("src/a.rs"), "{changes:?}");
    }

    #[test]
    fn a_new_file_is_a_change() {
        let tmp = tree();
        let before = SourceScan::take(&[tmp.path().to_path_buf()]).expect("first");
        fs::write(tmp.path().join("src/c.rs"), "fn c() {}").expect("new");
        let after = SourceScan::take(&[tmp.path().to_path_buf()]).expect("second");
        let changes = before.changes(&after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(changes[0].ends_with("src/c.rs"), "{changes:?}");
    }

    #[test]
    fn a_deleted_file_is_a_change() {
        let tmp = tree();
        let before = SourceScan::take(&[tmp.path().to_path_buf()]).expect("first");
        fs::remove_file(tmp.path().join("src/nested/b.rs")).expect("delete");
        let after = SourceScan::take(&[tmp.path().to_path_buf()]).expect("second");
        let changes = before.changes(&after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(changes[0].ends_with("src/nested/b.rs"), "{changes:?}");
    }

    #[test]
    fn writing_build_output_is_not_a_change() {
        let tmp = tree();
        let before = SourceScan::take(&[tmp.path().to_path_buf()]).expect("first");
        fs::write(tmp.path().join("target/debug/out"), "rebuilt binary").expect("rebuild");
        fs::write(tmp.path().join("dist/app.js"), "// rebuilt").expect("rebundle");
        let after = SourceScan::take(&[tmp.path().to_path_buf()]).expect("second");
        assert!(before.changes(&after).is_empty());
    }

    #[test]
    fn a_single_file_root_is_watched_directly() {
        let tmp = tree();
        let lock = tmp.path().join("Cargo.lock");
        fs::write(&lock, "# lock").expect("lock");
        let scan = SourceScan::take(std::slice::from_ref(&lock)).expect("scanned");
        assert_eq!(scan.len(), 1);
    }

    #[test]
    fn a_missing_root_is_a_typed_error() {
        use fundamental_core::Error as _;
        let err = SourceScan::take(&[PathBuf::from("/definitely/not/here")]).expect_err("missing");
        assert_eq!(err.error_code(), "DEV_001");
    }
}
