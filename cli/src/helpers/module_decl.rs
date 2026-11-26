//! The [`ensure_module_decl`] helper: append a `mod` declaration to a `.rs` file.

use std::fs;
use std::path::Path;

use tracing::{debug, instrument};

use crate::CliError;

/// Ensure the Rust source at `path` has the whole line `decl`, appending it (or creating the file).
///
/// # Errors
/// [`CliError::Io`] if the file cannot be read or written.
#[instrument(skip_all, fields(path = %path.display(), decl), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn ensure_module_decl(path: &Path, decl: &str) -> Result<(), CliError> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(CliError::Io {
                path: path.display().to_string(),
                reason: e.to_string(),
            });
        }
    };

    if existing.lines().any(|line| line.trim() == decl) {
        debug!(decl, "module declaration already present; no change");
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CliError::Io {
            path: parent.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(decl);
    next.push('\n');

    fs::write(path, next).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    debug!(decl, "appended module declaration");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn appends_to_an_existing_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("lib.rs");
        fs::write(&path, "//! crate\n\npub struct A;\n").unwrap();
        ensure_module_decl(&path, "pub mod models;").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("pub struct A;"), "{out}");
        assert!(out.contains("pub mod models;"), "{out}");
    }

    #[test]
    fn is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("mod.rs");
        ensure_module_decl(&path, "pub mod post;").unwrap();
        ensure_module_decl(&path, "pub mod post;").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert_eq!(out.matches("pub mod post;").count(), 1, "{out}");
    }

    #[test]
    fn creates_the_file_when_missing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("models").join("mod.rs");
        ensure_module_decl(&path, "pub mod post;").unwrap();
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "pub mod post;\n");
    }

    #[test]
    fn a_substring_match_does_not_suppress_the_append() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("mod.rs");
        fs::write(&path, "pub mod posts;\n").unwrap();
        ensure_module_decl(&path, "pub mod post;").unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("pub mod posts;"), "{out}");
        assert!(out.contains("pub mod post;"), "{out}");
    }
}
