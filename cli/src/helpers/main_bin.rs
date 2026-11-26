//! [`resolve_main_bin()`]: the binary that *is* an app, as opposed to its tooling.

use std::fs;

use tracing::{debug, instrument};

use crate::helpers::{Project, bin_names};
use crate::{CliError, CliResult};

const ROLES: &[&str] = &["migrate", "commands"];

/// Resolve the `[[bin]]` that runs `app` itself: its own name, else the one non-tooling binary.
///
/// # Errors
/// [`CliError::Io`] if the package manifest cannot be read, [`CliError::AppBinMissing`] when no
/// candidate, or more than one, remains.
#[instrument(skip(project), fields(app), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn resolve_main_bin(project: &Project, app: &str) -> CliResult<String> {
    let manifest_path = project.app_dir(app).join("Cargo.toml");
    let text = fs::read_to_string(&manifest_path).map_err(|e| CliError::Io {
        path: manifest_path.display().to_string(),
        reason: e.to_string(),
    })?;

    let crate_name = app.replace('-', "_");
    let declared = bin_names(&text);

    if declared.iter().any(|name| name == &crate_name) {
        debug!(bin = %crate_name, "the app's binary shares its name");
        return Ok(crate_name);
    }

    let candidates: Vec<&String> = declared
        .iter()
        .filter(|name| !is_role_bin(name, app, &crate_name))
        .collect();

    if let [only] = candidates.as_slice() {
        debug!(bin = %only, "the app's only non-tooling binary");
        return Ok((*only).clone());
    }

    Err(CliError::AppBinMissing {
        app: app.to_string(),
        role: if candidates.is_empty() {
            "application".to_string()
        } else {
            format!(
                "application (several candidates: {})",
                candidates
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        prefixed: crate_name,
        manifest: manifest_path.display().to_string(),
    })
}

/// Whether `bin` is a tooling binary, prefixed or bare.
fn is_role_bin(bin: &str, app: &str, crate_name: &str) -> bool {
    ROLES.iter().any(|role| {
        bin == *role || bin == format!("{app}-{role}") || bin == format!("{crate_name}-{role}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn project_with(app: &str, bins: &[&str]) -> (TempDir, Project) {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("fun.toml"), "[project]\nname = \"p\"\n").expect("manifest");
        let app_dir = tmp.path().join("apps").join(app);
        fs::create_dir_all(&app_dir).expect("app dir");
        let body: String = bins
            .iter()
            .map(|b| format!("[[bin]]\nname = \"{b}\"\n\n"))
            .collect();
        fs::write(app_dir.join("Cargo.toml"), body).expect("crate manifest");
        let project = Project::discover_from(tmp.path()).expect("discoverable");
        (tmp, project)
    }

    #[test]
    fn the_binary_named_after_the_app_wins() {
        let (_t, project) =
            project_with("layer0", &["layer0", "layer0-migrate", "layer0-commands"]);
        assert_eq!(
            resolve_main_bin(&project, "layer0").expect("resolved"),
            "layer0"
        );
    }

    #[test]
    fn an_app_whose_binary_is_named_differently_still_resolves() {
        let (_t, project) = project_with(
            "layer5_witness",
            &[
                "witness",
                "layer5_witness-migrate",
                "layer5_witness-commands",
            ],
        );
        assert_eq!(
            resolve_main_bin(&project, "layer5_witness").expect("resolved"),
            "witness"
        );
    }

    #[test]
    fn the_bare_role_spellings_are_excluded_too() {
        let (_t, project) = project_with("layer5_mirror", &["mirror", "migrate"]);
        assert_eq!(
            resolve_main_bin(&project, "layer5_mirror").expect("resolved"),
            "mirror"
        );
    }

    #[test]
    fn an_ambiguous_package_is_refused_and_names_the_candidates() {
        use fundamental::core::Error as _;
        let (_t, project) = project_with("layer5", &["serve", "keeper", "layer5-migrate"]);
        let err = resolve_main_bin(&project, "layer5").expect_err("ambiguous");
        assert_eq!(err.error_code(), "CLI_041");
        assert!(format!("{err}").contains("serve"), "{err}");
        assert!(format!("{err}").contains("keeper"), "{err}");
    }

    #[test]
    fn a_package_with_only_tooling_binaries_is_an_error() {
        use fundamental::core::Error as _;
        let (_t, project) = project_with("layer0", &["layer0-migrate"]);
        let err = resolve_main_bin(&project, "layer0").expect_err("no app binary");
        assert_eq!(err.error_code(), "CLI_041");
    }

    #[test]
    fn a_hyphenated_app_resolves_its_underscored_crate_name() {
        let (_t, project) = project_with("holder-vault", &["holder_vault"]);
        assert_eq!(
            resolve_main_bin(&project, "holder-vault").expect("resolved"),
            "holder_vault"
        );
    }
}
