//! [`tree_entries`]: every file under a directory, as package entries under one packed path.

use std::fs;
use std::path::Path;

use fundamental_package::{FileMode, PackageEntry, PackedPath};

use crate::{CliError, CliResult};

/// Every file under `dir` in path order, at `under/<path>`; a link or special file is kept, to be refused.
///
/// # Errors
/// [`CliError::Io`] when a directory cannot be read, or a path [`PackedPath::parse`] refuses.
pub(crate) fn tree_entries(dir: &Path, under: &str) -> CliResult<Vec<PackageEntry>> {
    let mut entries = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }
    walk(dir, under, &mut entries)?;
    Ok(entries)
}

fn walk(dir: &Path, under: &str, entries: &mut Vec<PackageEntry>) -> CliResult<()> {
    let io = |e: std::io::Error| CliError::Io {
        path: dir.display().to_string(),
        reason: e.to_string(),
    };
    let mut children: Vec<_> = fs::read_dir(dir)
        .map_err(io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io)?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let name = child.file_name();
        let packed = format!("{under}/{}", name.to_string_lossy());
        if child.file_type().map_err(io)?.is_dir() {
            walk(&child.path(), &packed, entries)?;
        } else {
            entries.push(PackageEntry {
                source: child.path(),
                path: PackedPath::parse(&packed)?,
                mode: FileMode::Plain,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_held_under_its_path_in_order() {
        let dir = tempfile::tempdir().expect("dir");
        fs::create_dir_all(dir.path().join("assets/fonts")).expect("dirs");
        for file in ["b.js", "a_bg.wasm", "assets/fonts/inter.woff2"] {
            fs::write(dir.path().join(file), b"x").expect("write");
        }

        let held: Vec<String> = tree_entries(dir.path(), "share/dist")
            .expect("walked")
            .into_iter()
            .map(|entry| entry.path.to_string())
            .collect();

        assert_eq!(
            held,
            [
                "share/dist/a_bg.wasm",
                "share/dist/assets/fonts/inter.woff2",
                "share/dist/b.js"
            ]
        );
    }

    #[test]
    fn a_directory_that_is_not_there_holds_nothing() {
        let dir = tempfile::tempdir().expect("dir");

        assert!(
            tree_entries(&dir.path().join("migrations"), "share/migrations")
                .expect("walked")
                .is_empty()
        );
    }

    #[test]
    fn a_file_name_a_package_cannot_hold_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        fs::write(dir.path().join("logo final.svg"), b"x").expect("write");

        let err = tree_entries(dir.path(), "share/dist").expect_err("a space");

        assert!(matches!(err, CliError::Package(_)), "{err:?}");
    }
}
