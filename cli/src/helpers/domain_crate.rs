//! [`DomainCrate`]: the shared domain crate an app depends on, wherever it lives.

use std::collections::BTreeSet;
use std::path::Path;

use tracing::debug;

use crate::CliError;
use crate::helpers::CrateManifest;

/// The domain crate a generated app depends on as `domain`: `domain/` or `domain/<name>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomainCrate {
    package: String,
    /// Relative to the workspace root, with `/` separators.
    dir: String,
    features: BTreeSet<String>,
}

impl DomainCrate {
    /// The crate `fun new` scaffolds at `domain/`, with the features its template declares.
    pub(crate) fn scaffolded() -> Self {
        Self {
            package: "domain".to_owned(),
            dir: "domain".to_owned(),
            features: ["server", "sync-server"].map(str::to_owned).into(),
        }
    }

    /// Reads the crate at `dir`, a directory under the workspace `root`.
    ///
    /// # Errors
    /// [`CliError::Io`] or [`CliError::TomlParse`] when its `Cargo.toml` cannot be read or names no package.
    pub(crate) fn read(root: &Path, dir: &Path) -> Result<Self, CliError> {
        let path = dir.join("Cargo.toml");
        let manifest = CrateManifest::load(&path)?;
        let package = manifest
            .package_name()
            .ok_or_else(|| CliError::TomlParse {
                path: path.display().to_string(),
                reason: "no [package] name".to_owned(),
            })?
            .to_owned();
        let features = manifest.feature_names().into_iter().collect();
        let dir = dir
            .strip_prefix(root)
            .unwrap_or(dir)
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        debug!(package, dir, "read the domain crate an app depends on");
        Ok(Self {
            package,
            dir,
            features,
        })
    }

    /// Refuses a feature the crate does not declare, which cargo would refuse later and less clearly.
    ///
    /// # Errors
    /// [`CliError::DomainFeatureMissing`] naming the crate and the feature.
    pub(crate) fn require(&self, features: &[&str]) -> Result<(), CliError> {
        match features
            .iter()
            .find(|feature| !self.features.contains(**feature))
        {
            None => Ok(()),
            Some(missing) => Err(CliError::DomainFeatureMissing {
                package: self.package.clone(),
                feature: (*missing).to_owned(),
            }),
        }
    }

    /// The `domain = { … }` line for a crate `depth` directories below the workspace root.
    pub(crate) fn dependency(&self, depth: usize, features: &[&str]) -> String {
        let path = format!("{}{}", "../".repeat(depth), self.dir);
        let package = if self.package == "domain" {
            String::new()
        } else {
            format!("package = \"{}\", ", self.package)
        };
        let features = if features.is_empty() {
            String::new()
        } else {
            let listed: Vec<String> = features
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect();
            format!(", features = [{}]", listed.join(", "))
        };
        format!("domain = {{ {package}path = \"{path}\"{features} }}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::TempDir;

    fn crate_at(root: &Path, dir: &str, manifest: &str) -> std::path::PathBuf {
        let path = root.join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("Cargo.toml"), manifest).unwrap();
        path
    }

    #[test]
    fn the_scaffolded_crate_is_written_as_it_always_was() {
        assert_eq!(
            DomainCrate::scaffolded().dependency(2, &["server"]),
            "domain = { path = \"../../domain\", features = [\"server\"] }"
        );
        assert_eq!(
            DomainCrate::scaffolded().dependency(3, &[]),
            "domain = { path = \"../../../domain\" }"
        );
    }

    #[test]
    fn a_split_domain_is_depended_on_by_its_package_and_its_own_path() {
        let root = TempDir::new().unwrap();
        let dir = crate_at(
            root.path(),
            "domain/shared",
            "[package]\nname = \"domain_shared\"\n[features]\nserver = []\n",
        );
        let domain = DomainCrate::read(root.path(), &dir).unwrap();
        assert_eq!(
            domain.dependency(2, &["server"]),
            "domain = { package = \"domain_shared\", path = \"../../domain/shared\", features = [\"server\"] }"
        );
        assert!(domain.require(&["server"]).is_ok());
    }

    #[test]
    fn a_feature_the_crate_does_not_declare_is_refused_up_front() {
        let root = TempDir::new().unwrap();
        let dir = crate_at(
            root.path(),
            "domain/shared",
            "[package]\nname = \"domain_shared\"\n",
        );
        let refused = DomainCrate::read(root.path(), &dir)
            .unwrap()
            .require(&["server", "sync-server"])
            .unwrap_err();
        assert!(matches!(
            &refused,
            CliError::DomainFeatureMissing { package, feature }
                if package == "domain_shared" && feature == "server"
        ));
    }
}
