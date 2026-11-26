//! Cross-check the declared surfaces against what the app directory actually holds.

use std::path::Path;

use tracing::{debug, instrument};

use crate::build_error::BuildError;

/// Verify the app layout matches its declared surfaces (pages <-> `web`).
///
/// # Errors
/// - [`BuildError::MissingPages`] if `web` is declared but `app_dir/pages/` is absent.
/// - [`BuildError::UnexpectedPages`] if `app_dir/pages/` exists but `web` is not declared.
#[instrument(skip_all, fields(app_dir = %app_dir.display()), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn check_app_layout(app_dir: &Path, declared_surfaces: &[String]) -> Result<(), BuildError> {
    let has_web = declared_surfaces.iter().any(|s| s == "web");
    let has_pages = app_dir.join("pages").is_dir();

    match (has_web, has_pages) {
        (true, false) => {
            debug!("web surface declared but pages dir missing");
            Err(BuildError::MissingPages {
                app: app_dir.to_path_buf(),
            })
        }
        (false, true) => {
            debug!("pages dir present but no web surface declared");
            Err(BuildError::UnexpectedPages {
                app: app_dir.to_path_buf(),
            })
        }
        _ => {
            debug!(has_web, has_pages, "app layout consistent");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;
    use std::fs;
    use tempfile::TempDir;

    fn web() -> Vec<String> {
        vec!["web".to_string()]
    }

    #[test]
    fn web_with_pages_is_ok() {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("pages")).unwrap();
        check_app_layout(d.path(), &web()).unwrap();
    }

    #[test]
    fn web_without_pages_errors() {
        let d = TempDir::new().unwrap();
        let err = check_app_layout(d.path(), &web()).unwrap_err();
        assert!(matches!(err, BuildError::MissingPages { .. }));
        assert_eq!(err.error_code(), "BUILD_006");
    }

    #[test]
    fn pages_without_web_errors() {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("pages")).unwrap();
        let err = check_app_layout(d.path(), &["api".to_string()]).unwrap_err();
        assert!(matches!(err, BuildError::UnexpectedPages { .. }));
        assert_eq!(err.error_code(), "BUILD_007");
    }

    #[test]
    fn no_web_no_pages_is_ok() {
        let d = TempDir::new().unwrap();
        check_app_layout(d.path(), &["api".to_string()]).unwrap();
    }
}
