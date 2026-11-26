use std::path::Path;

use fundamental_manifest::{Manifest, ManifestError};
use tracing::debug;

use crate::{ConfigError, ConfigResult};

/// The settings of the project whose `fun.toml` is `path`, as development reads them.
///
/// # Errors
/// [`ConfigError::NotAManifest`], [`ConfigError::ReadFile`] when a required manifest is missing,
/// or [`ConfigError::Manifest`].
pub(crate) fn project_manifest(path: &Path, required: bool) -> ConfigResult<Option<Manifest>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(Manifest::FILE) {
        return Err(ConfigError::NotAManifest {
            path: path.to_path_buf(),
        });
    }
    let root = path.parent().unwrap_or_else(|| Path::new(""));
    match Manifest::load(root) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(ManifestError::NotFound { .. }) if !required => {
            debug!(path = %path.display(), "no manifest; it declares nothing");
            Ok(None)
        }
        Err(ManifestError::NotFound { .. }) => Err(ConfigError::ReadFile {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        }),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn reads_the_project_and_this_machines_file() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        fs::write(
            project.path().join("fun.toml"),
            "[project]\nname = \"x\"\n[apps.main]\nweb = { port = 1 }\n",
        )?;
        fs::write(
            project.path().join("fun.local.toml"),
            "[apps.main.web]\nport = 2\n",
        )?;
        let manifest =
            project_manifest(&project.path().join("fun.toml"), true)?.ok_or("a manifest")?;
        assert_eq!(manifest.files().len(), 2);
        Ok(())
    }

    #[test]
    fn refuses_a_file_that_is_not_a_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        let elsewhere = project.path().join("elsewhere.toml");
        fs::write(&elsewhere, "[project]\nname = \"x\"\n")?;
        let refused = project_manifest(&elsewhere, true);
        assert!(
            matches!(&refused, Err(error @ ConfigError::NotAManifest { .. }) if error.error_code() == "CFG_022"),
            "{refused:?}"
        );
        Ok(())
    }

    #[test]
    fn a_missing_manifest_is_absent_unless_required() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        let path = project.path().join("fun.toml");
        assert!(project_manifest(&path, false)?.is_none());
        assert!(matches!(
            project_manifest(&path, true),
            Err(ConfigError::ReadFile { .. })
        ));
        Ok(())
    }
}
