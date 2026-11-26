//! [`resolve_app_bin()`]: the binary name for a conventional per-app role.

use std::fs;

use tracing::{debug, instrument};

use crate::helpers::Project;
use crate::{CliError, CliResult};

/// Resolve the `[[bin]]` carrying `role` for `app`: `<app>-<role>`, which cannot collide, else `<role>`.
///
/// # Errors
/// [`CliError::Io`] if the package manifest cannot be read, [`CliError::AppBinMissing`] for neither.
#[instrument(skip(project), fields(app, role), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn resolve_app_bin(project: &Project, app: &str, role: &str) -> CliResult<String> {
    let manifest_path = project.app_dir(app).join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path).map_err(|e| CliError::Io {
        path: manifest_path.display().to_string(),
        reason: e.to_string(),
    })?;

    let declared = bin_names(&text);
    let prefixed = format!("{app}-{role}");

    if declared.iter().any(|name| name == &prefixed) {
        debug!(bin = %prefixed, "resolved the prefixed role binary");
        return Ok(prefixed);
    }
    if declared.iter().any(|name| name == role) {
        debug!(bin = role, "resolved the bare role binary");
        return Ok(role.to_string());
    }

    Err(CliError::AppBinMissing {
        app: app.to_string(),
        role: role.to_string(),
        prefixed,
        manifest: manifest_path.display().to_string(),
    })
}

/// The `name` of every `[[bin]]` target, parsed so `[package]`/`[[bench]]` names are not taken.
pub(crate) fn bin_names(manifest: &str) -> Vec<String> {
    let Ok(doc) = manifest.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(bins) = doc.get("bin").and_then(|item| item.as_array_of_tables()) else {
        return Vec::new();
    };
    bins.iter()
        .filter_map(|table| table.get("name")?.as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn project_with(app: &str, body: &str) -> (TempDir, Project) {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("fun.toml"), "[project]\nname = \"p\"\n").expect("manifest");
        let app_dir = tmp.path().join("apps").join(app);
        fs::create_dir_all(&app_dir).expect("app dir");
        fs::write(app_dir.join("Cargo.toml"), body).expect("crate manifest");
        let project = Project::discover_from(tmp.path()).expect("discoverable");
        (tmp, project)
    }

    #[test]
    fn prefers_the_prefixed_binary() {
        let (_t, project) = project_with(
            "layer0",
            "[package]\nname = \"layer0\"\n\n[[bin]]\nname = \"layer0-migrate\"\npath = \"src/bin/migrate.rs\"\n",
        );
        assert_eq!(
            resolve_app_bin(&project, "layer0", "migrate").expect("resolved"),
            "layer0-migrate"
        );
    }

    #[test]
    fn falls_back_to_the_bare_role_for_a_single_app_project() {
        let (_t, project) = project_with(
            "main",
            "[package]\nname = \"main\"\n\n[[bin]]\nname = \"migrate\"\npath = \"src/bin/migrate.rs\"\n",
        );
        assert_eq!(
            resolve_app_bin(&project, "main", "migrate").expect("resolved"),
            "migrate"
        );
    }

    #[test]
    fn the_prefixed_name_wins_when_both_are_declared() {
        let (_t, project) = project_with(
            "layer0",
            "[package]\nname = \"layer0\"\n\n[[bin]]\nname = \"migrate\"\n\n[[bin]]\nname = \"layer0-migrate\"\n",
        );
        assert_eq!(
            resolve_app_bin(&project, "layer0", "migrate").expect("resolved"),
            "layer0-migrate"
        );
    }

    #[test]
    fn a_package_declaring_neither_names_both_candidates() {
        let (_t, project) = project_with(
            "layer0",
            "[package]\nname = \"layer0\"\n\n[[bin]]\nname = \"layer0\"\n",
        );
        let err = resolve_app_bin(&project, "layer0", "migrate").expect_err("no migrate binary");
        let CliError::AppBinMissing { role, prefixed, .. } = &err else {
            panic!("expected AppBinMissing, got {err:?}");
        };
        assert_eq!(role, "migrate");
        assert_eq!(prefixed, "layer0-migrate");
    }

    #[test]
    fn a_missing_manifest_is_an_io_error_naming_the_path() {
        let (_t, project) = project_with("layer0", "[package]\nname = \"layer0\"\n");
        let err = resolve_app_bin(&project, "absent", "migrate").expect_err("no such app");
        assert!(matches!(err, CliError::Io { .. }), "{err:?}");
    }

    #[test]
    fn only_bin_tables_are_considered() {
        let names = bin_names(
            "[package]\nname = \"layer0\"\n\n[[bench]]\nname = \"migrate\"\n\n[[bin]]\nname = \"layer0-commands\"\n",
        );
        assert_eq!(names, vec!["layer0-commands".to_string()]);
    }

    #[test]
    fn an_unparseable_manifest_declares_no_binaries() {
        assert!(bin_names("[[bin]\nname = ").is_empty());
    }
}
