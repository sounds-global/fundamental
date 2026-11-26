//! The public-assets step of the bundle build: the project's `assets/`, then the app's over it.

use std::path::Path;

use crate::ui::Ui;
use crate::{CliError, CliResult};

/// Copy `<project_root>/assets/`, then `<app_dir>/assets/` over it, into `<out_dir>/assets/`.
///
/// # Errors
/// [`CliError::Io`] if a directory or file cannot be read or written.
pub(crate) fn copy_assets(
    project_root: &Path,
    app_dir: &Path,
    out_dir: &Path,
    ui: &Ui,
) -> CliResult<()> {
    let shared = project_root.join("assets");
    let own = app_dir.join("assets");
    if !shared.is_dir() && !own.is_dir() {
        return Ok(());
    }
    ui.section("Copying public assets");
    let target = out_dir.join("assets");
    let mut copied = 0;
    for source in [&shared, &own] {
        if source.is_dir() {
            copied += copy_dir(source, &target)?;
        }
    }
    ui.detail(format!("{copied} file(s) into {}", target.display()));
    Ok(())
}

/// Recursively copy `source` into `target`, overwriting, and answer how many files were copied.
fn copy_dir(source: &Path, target: &Path) -> CliResult<u64> {
    std::fs::create_dir_all(target).map_err(|e| CliError::Io {
        path: target.display().to_string(),
        reason: format!("could not create the assets directory: {e}"),
    })?;
    let entries = std::fs::read_dir(source).map_err(|e| CliError::Io {
        path: source.display().to_string(),
        reason: format!("could not read the assets directory: {e}"),
    })?;
    let mut copied = 0;
    for entry in entries {
        let entry = entry.map_err(|e| CliError::Io {
            path: source.display().to_string(),
            reason: format!("could not read an assets directory entry: {e}"),
        })?;
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copied += copy_dir(&path, &destination)?;
        } else {
            std::fs::copy(&path, &destination).map_err(|e| CliError::Io {
                path: path.display().to_string(),
                reason: format!("could not copy the asset: {e}"),
            })?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::copy_assets;
    use crate::ui::Ui;

    fn write_asset(dir: &std::path::Path, relative: &str, contents: &[u8]) {
        let path = dir.join("assets").join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create source tree");
        }
        std::fs::write(path, contents).expect("write asset");
    }

    fn read_copy(out_dir: &std::path::Path, relative: &str) -> Vec<u8> {
        std::fs::read(out_dir.join("assets").join(relative)).expect("read copy")
    }

    #[test]
    fn absent_assets_directory_is_a_no_op() {
        let project = tempfile::tempdir().expect("tempdir");
        let app_dir = tempfile::tempdir().expect("tempdir");
        let out_dir = tempfile::tempdir().expect("tempdir");
        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("no-op");
        assert!(!out_dir.path().join("assets").exists());
    }

    #[test]
    fn assets_copy_recursively_into_out_dir() {
        let project = tempfile::tempdir().expect("tempdir");
        let app_dir = tempfile::tempdir().expect("tempdir");
        let out_dir = tempfile::tempdir().expect("tempdir");
        write_asset(app_dir.path(), "fonts/Inter.woff2", b"font-bytes");
        write_asset(app_dir.path(), "favicon.svg", b"<svg/>");

        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("copy");

        assert_eq!(
            read_copy(out_dir.path(), "fonts/Inter.woff2"),
            b"font-bytes"
        );
        assert!(out_dir.path().join("assets").join("favicon.svg").exists());
    }

    #[test]
    fn rebuild_overwrites_stale_assets() {
        let project = tempfile::tempdir().expect("tempdir");
        let app_dir = tempfile::tempdir().expect("tempdir");
        let out_dir = tempfile::tempdir().expect("tempdir");
        write_asset(app_dir.path(), "favicon.svg", b"v1");
        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("first copy");

        write_asset(app_dir.path(), "favicon.svg", b"v2");
        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("second copy");

        assert_eq!(read_copy(out_dir.path(), "favicon.svg"), b"v2");
    }

    #[test]
    fn the_projects_shared_assets_ship_even_when_the_app_has_none() {
        let project = tempfile::tempdir().expect("tempdir");
        let app_dir = tempfile::tempdir().expect("tempdir");
        let out_dir = tempfile::tempdir().expect("tempdir");
        write_asset(project.path(), "brand/wordmark.svg", b"<svg/>");

        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("copy");

        assert_eq!(read_copy(out_dir.path(), "brand/wordmark.svg"), b"<svg/>");
    }

    #[test]
    fn an_app_asset_overrides_the_projects_file_of_the_same_name() {
        let project = tempfile::tempdir().expect("tempdir");
        let app_dir = tempfile::tempdir().expect("tempdir");
        let out_dir = tempfile::tempdir().expect("tempdir");
        write_asset(project.path(), "brand/favicon.svg", b"shared");
        write_asset(project.path(), "brand/wordmark.svg", b"shared");
        write_asset(app_dir.path(), "brand/favicon.svg", b"app-specific");

        copy_assets(
            project.path(),
            app_dir.path(),
            out_dir.path(),
            &Ui::new(true),
        )
        .expect("copy");

        // A per-file overlay, not a per-directory swap.
        assert_eq!(
            read_copy(out_dir.path(), "brand/favicon.svg"),
            b"app-specific"
        );
        assert_eq!(read_copy(out_dir.path(), "brand/wordmark.svg"), b"shared");
    }
}
