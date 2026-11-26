//! [`up_file`] — the one place a migration's name becomes a path.

use std::path::{Path, PathBuf};

use fundamental_db::migration::{MigrationError, MigrationResult};

/// The characters a migration name may contain: what `fun migrate create` writes
/// (`<timestamp>_<slug>`) plus the separators a hand-named one plausibly uses.
fn is_legal(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// Whether `name` may name a migration, or the owner of an embedded set: non-empty, not
/// `..`, and nothing outside `[A-Za-z0-9._-]` — so never a path separator.
pub(crate) fn is_legal_name(name: &str) -> bool {
    !name.is_empty() && name != ".." && name.chars().all(is_legal)
}

/// Resolves `<dir>/<name>.up.<ext>`, refusing any `name` that is not a single,
/// ordinary path component.
///
/// # Errors
/// Returns [`MigrationError::UnsafeName`] when `name` is empty, contains anything outside
/// `[A-Za-z0-9._-]`, or would not survive as its own file name.
pub(crate) fn up_file(dir: &Path, name: &str, ext: &str) -> MigrationResult<PathBuf> {
    if !is_legal_name(name) {
        return Err(MigrationError::UnsafeName {
            name: name.to_owned(),
        });
    }

    let file_name = format!("{name}.up.{ext}");
    let path = dir.join(&file_name);

    // Whatever the platform makes of the join, the result must be `dir`'s own child by that name.
    if path.file_name().and_then(|n| n.to_str()) != Some(file_name.as_str())
        || path.parent() != Some(dir)
    {
        return Err(MigrationError::UnsafeName {
            name: name.to_owned(),
        });
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::up_file;
    use std::path::Path;

    #[test]
    fn a_real_migration_name_resolves_under_its_directory() {
        let dir = Path::new("/apps/layer0/migrations/sql");
        let path = up_file(dir, "20260816120000_create_widgets", "sql").expect("legal name");

        assert_eq!(
            path,
            Path::new("/apps/layer0/migrations/sql/20260816120000_create_widgets.up.sql")
        );
    }

    #[test]
    fn a_name_that_navigates_is_refused_rather_than_followed() {
        let dir = Path::new("/apps/layer0/migrations/sql");

        for escape in [
            "../../../etc/passwd",
            "..",
            "/etc/passwd",
            "sub/dir",
            "",
            "name with spaces",
        ] {
            assert!(
                up_file(dir, escape, "sql").is_err(),
                "{escape:?} must not resolve to a path"
            );
        }
    }
}
