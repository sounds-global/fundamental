//! Seed-file discovery: list `seeds/*.rs` (excluding `mod.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

/// Discover seed source files under `app_dir/seeds/` except `mod.rs`, sorted, relative to
/// `app_dir`; a missing or unreadable directory yields none.
#[must_use]
#[instrument(skip_all, fields(app_dir = %app_dir.display()))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn discover_seed_files(app_dir: &Path) -> Vec<PathBuf> {
    let root = app_dir.join("seeds");
    let Ok(read) = fs::read_dir(&root) else {
        debug!("no seeds directory; empty set");
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy() != "mod.rs")
        })
        .map(|p| p.strip_prefix(app_dir).unwrap_or(&p).to_path_buf())
        .collect();

    files.sort();
    debug!(count = files.len(), "discovered seed files");
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(dir: &Path, rel: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "// seed").unwrap();
    }

    #[test]
    fn missing_dir_is_empty() {
        let d = TempDir::new().unwrap();
        assert!(discover_seed_files(d.path()).is_empty());
    }

    #[test]
    fn lists_rs_files_excluding_mod_sorted() {
        let d = TempDir::new().unwrap();
        touch(d.path(), "seeds/zebra.rs");
        touch(d.path(), "seeds/alpha.rs");
        touch(d.path(), "seeds/mod.rs");
        touch(d.path(), "seeds/notes.txt");
        let files = discover_seed_files(d.path());
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], PathBuf::from("seeds/alpha.rs"));
        assert_eq!(files[1], PathBuf::from("seeds/zebra.rs"));
    }
}
