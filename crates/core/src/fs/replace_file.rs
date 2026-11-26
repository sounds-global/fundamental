use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::warn;

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Replaces the file at `target` with `bytes`, so a reader sees the old file or the new one.
///
/// Written to a dot-file beside `target`, flushed and renamed over it; a symbolic link is refused.
///
/// # Errors
///
/// [`io::ErrorKind::InvalidInput`] when `target` has no parent or file name or is a symbolic link,
/// otherwise the error of the write, flush or rename; the dot-file is removed on failure.
pub fn replace_file(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a replaced file needs a parent directory and a name",
        ));
    };
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a symbolic link is never replaced",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let temporary = parent.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let written = write_new(&temporary, bytes).and_then(|()| fs::rename(&temporary, target));
    if let Err(error) = written {
        if let Err(cleanup) = fs::remove_file(&temporary) {
            warn!(path = %temporary.display(), error = %cleanup, "temporary file not removed");
        }
        return Err(error);
    }
    sync_directory(parent)
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    fs::File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_new_file() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("a.toml");
        replace_file(&target, b"x = 1\n")?;
        assert_eq!(fs::read(&target)?, b"x = 1\n");
        Ok(())
    }

    #[test]
    fn replaces_an_existing_file_and_leaves_no_temporary() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("a.toml");
        fs::write(&target, b"old")?;
        replace_file(&target, b"new")?;
        assert_eq!(fs::read(&target)?, b"new");
        let names: Vec<_> = fs::read_dir(directory.path())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<_>>()?;
        assert_eq!(names, vec![std::ffi::OsString::from("a.toml")]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symbolic_link() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let elsewhere = directory.path().join("elsewhere");
        fs::write(&elsewhere, b"keep")?;
        let target = directory.path().join("a.toml");
        std::os::unix::fs::symlink(&elsewhere, &target)?;
        let refused = replace_file(&target, b"new");
        assert_eq!(
            refused.map_err(|e| e.kind()),
            Err(io::ErrorKind::InvalidInput)
        );
        assert_eq!(fs::read(&elsewhere)?, b"keep");
        Ok(())
    }

    #[test]
    fn refuses_a_missing_parent() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("absent").join("a.toml");
        assert_eq!(
            replace_file(&target, b"x").map_err(|e| e.kind()),
            Err(io::ErrorKind::NotFound)
        );
        Ok(())
    }
}
