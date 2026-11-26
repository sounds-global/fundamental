use std::fs::{self, File};
use std::io;
use std::path::Path;

/// Opens the regular file at `path` for reading, never through a symbolic link.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] for a symbolic link, a directory, a special file, or a file
/// swapped in before opening; otherwise the error of reading its metadata or opening it.
pub fn open_regular(path: &Path) -> io::Result<File> {
    let refused = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "only a regular file is opened, never through a symbolic link",
        )
    };
    let named = fs::symlink_metadata(path)?;
    if !named.file_type().is_file() {
        return Err(refused());
    }
    let file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.file_type().is_file() || !same_file(&named, &opened) {
        return Err(refused());
    }
    Ok(file)
}

#[cfg(unix)]
fn same_file(named: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    (named.dev(), named.ino()) == (opened.dev(), opened.ino())
}

#[cfg(not(unix))]
fn same_file(named: &fs::Metadata, opened: &fs::Metadata) -> bool {
    named.len() == opened.len() && named.modified().ok() == opened.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_regular_file_opens() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("desired.json");
        fs::write(&path, b"{}").expect("write");

        let opened = open_regular(&path).expect("opened");

        assert_eq!(opened.metadata().expect("metadata").len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_or_a_directory_is_not_opened() {
        let dir = tempfile::tempdir().expect("dir");
        let target = dir.path().join("shadow");
        fs::write(&target, b"not this").expect("write");
        let link = dir.path().join("desired.json");
        std::os::unix::fs::symlink(&target, &link).expect("link");

        for path in [link.as_path(), dir.path()] {
            let err = open_regular(path).expect_err("refused");
            assert_eq!(
                err.kind(),
                io::ErrorKind::InvalidInput,
                "{}",
                path.display()
            );
        }
    }
}
