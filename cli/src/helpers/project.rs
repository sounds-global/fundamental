//! The [`Project`] handle: the root of the Fundamental project a command operates on.

use std::env;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

use crate::CliError;

const MARKER: &str = fundamental_manifest::Manifest::FILE;

/// A located Fundamental project: the directory containing `fun.toml`.
#[derive(Debug, Clone)]
pub(crate) struct Project {
    root: PathBuf,
}

impl Project {
    /// Locate the project by walking up from the current directory to the first holding `fun.toml`.
    ///
    /// # Errors
    /// [`CliError::NotInProject`] if no ancestor holds one, [`CliError::Io`] if the cwd cannot be read.
    #[instrument(err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn discover() -> Result<Self, CliError> {
        let start = env::current_dir().map_err(|e| CliError::Io {
            path: ".".to_string(),
            reason: e.to_string(),
        })?;
        Self::discover_from(&start)
    }

    /// Walk up from `start` to the first directory containing `fun.toml`.
    ///
    /// # Errors
    /// [`CliError::NotInProject`] if neither `start` nor any ancestor holds one.
    pub(crate) fn discover_from(start: &Path) -> Result<Self, CliError> {
        let root = fundamental_manifest::Manifest::find_root(start).map_err(|cause| {
            debug!(start = %start.display(), %cause, "no project root");
            CliError::NotInProject {
                start: start.display().to_string(),
            }
        })?;
        debug!(root = %root.display(), "located project root");
        Ok(Self { root })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn manifest_path(&self) -> PathBuf {
        self.root.join(MARKER)
    }

    pub(crate) fn workspace_manifest_path(&self) -> PathBuf {
        self.root.join("Cargo.toml")
    }

    /// `apps/<app>`, whether or not it exists.
    pub(crate) fn app_dir(&self, app: &str) -> PathBuf {
        self.root.join("apps").join(app)
    }

    /// `domain/`, whether or not it exists: a crate itself, a directory of crates, or absent.
    pub(crate) fn domain_dir(&self) -> PathBuf {
        self.root.join("domain")
    }

    /// The project's domain crates, `domain/` or `domain/<name>/`, as `(name, directory)` sorted by name.
    pub(crate) fn domain_crates(&self) -> Vec<(String, PathBuf)> {
        let domain = self.domain_dir();
        if domain.join("Cargo.toml").is_file() {
            return vec![("domain".to_string(), domain)];
        }
        let Ok(entries) = std::fs::read_dir(&domain) else {
            debug!(domain = %domain.display(), "no domain directory");
            return Vec::new();
        };
        let mut crates: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("Cargo.toml").is_file())
            .filter_map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| (n.to_string(), p.clone()))
            })
            .collect();
        crates.sort_by(|a, b| a.0.cmp(&b.0));
        crates
    }

    /// Resolve which domain crate a generator writes into; several require `--crate`.
    ///
    /// # Errors
    /// [`CliError::NoDomainCrate`] when there is none, [`CliError::AmbiguousDomainCrate`] for several unnamed.
    pub(crate) fn resolve_domain_crate(&self, wanted: Option<&str>) -> Result<PathBuf, CliError> {
        let crates = self.domain_crates();
        let names = || {
            crates
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match (wanted, crates.len()) {
            (_, 0) => Err(CliError::NoDomainCrate {
                domain: self.domain_dir().display().to_string(),
            }),
            (None, 1) => Ok(crates[0].1.clone()),
            (None, _) => Err(CliError::AmbiguousDomainCrate { crates: names() }),
            (Some(w), _) => crates
                .iter()
                .find(|(n, _)| n == w)
                .map(|(_, d)| d.clone())
                .ok_or_else(|| CliError::AmbiguousDomainCrate { crates: names() }),
        }
    }

    /// The apps that define a `seeds/` directory, sorted.
    pub(crate) fn apps_with_seeds(&self) -> Vec<String> {
        self.apps_matching(|dir| dir.join("seeds").is_dir())
    }

    /// The apps that ship a `commands` binary (`src/bin/commands.rs` or `src/bin/commands/main.rs`), sorted.
    pub(crate) fn apps_with_commands(&self) -> Vec<String> {
        self.apps_matching(|dir| {
            let bin = dir.join("src").join("bin");
            bin.join("commands.rs").is_file() || bin.join("commands").join("main.rs").is_file()
        })
    }

    /// The apps that declare a migration binary (`<app>-migrate`, or `migrate`), sorted.
    pub(crate) fn apps_with_migrator(&self) -> Vec<String> {
        self.apps_matching(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|app| super::resolve_app_bin(self, app, "migrate").is_ok())
        })
    }

    fn apps_matching(&self, predicate: impl Fn(&Path) -> bool) -> Vec<String> {
        let apps_dir = self.root.join("apps");
        let Ok(entries) = std::fs::read_dir(&apps_dir) else {
            debug!(apps_dir = %apps_dir.display(), "no apps directory");
            return Vec::new();
        };
        let mut apps: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| predicate(&e.path()))
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        apps.sort();
        apps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discovers_root_from_a_nested_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join(MARKER), "[project]\n").unwrap();
        let nested = root.join("apps").join("main").join("src");
        fs::create_dir_all(&nested).unwrap();

        let project = Project::discover_from(&nested).expect("should find the root");
        assert_eq!(
            fs::canonicalize(project.manifest_path()).unwrap(),
            fs::canonicalize(root.join(MARKER)).unwrap()
        );
    }

    #[test]
    fn errors_when_no_marker_is_found() {
        let tmp = TempDir::new().unwrap();
        let err = Project::discover_from(tmp.path()).expect_err("no marker present");
        assert!(matches!(err, CliError::NotInProject { .. }));
    }

    #[test]
    fn app_dir_is_under_apps() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert_eq!(project.app_dir("billing"), tmp.path().join("apps/billing"));
    }

    #[test]
    fn domain_crates_reads_the_single_crate_shape() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert!(project.domain_crates().is_empty());

        fs::create_dir_all(tmp.path().join("domain")).unwrap();
        fs::write(tmp.path().join("domain/Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(
            project.domain_crates(),
            vec![("domain".to_string(), tmp.path().join("domain"))]
        );
        assert_eq!(
            project.resolve_domain_crate(None).unwrap(),
            tmp.path().join("domain")
        );
    }

    #[test]
    fn domain_crates_reads_the_many_crate_shape_and_asks_which() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        for name in ["layer0", "layer1", "shared"] {
            fs::create_dir_all(tmp.path().join("domain").join(name)).unwrap();
            fs::write(
                tmp.path().join("domain").join(name).join("Cargo.toml"),
                "[package]\n",
            )
            .unwrap();
        }
        let found: Vec<String> = project
            .domain_crates()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(found, ["layer0", "layer1", "shared"]);

        assert!(project.resolve_domain_crate(None).is_err());
        assert_eq!(
            project.resolve_domain_crate(Some("layer1")).unwrap(),
            tmp.path().join("domain/layer1")
        );
        assert!(project.resolve_domain_crate(Some("nope")).is_err());
    }

    #[test]
    fn resolve_domain_crate_refuses_when_there_is_none() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert!(project.resolve_domain_crate(None).is_err());
    }

    #[test]
    fn apps_with_seeds_lists_only_apps_that_have_a_seeds_dir() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        fs::create_dir_all(tmp.path().join("apps/main/seeds")).unwrap();
        fs::create_dir_all(tmp.path().join("apps/worker/src")).unwrap();
        fs::create_dir_all(tmp.path().join("apps/billing/seeds")).unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert_eq!(project.apps_with_seeds(), vec!["billing", "main"]);
    }

    #[test]
    fn apps_with_seeds_is_empty_without_an_apps_dir() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert!(project.apps_with_seeds().is_empty());
    }

    #[test]
    fn apps_with_commands_lists_only_apps_that_ship_a_commands_bin() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        fs::create_dir_all(tmp.path().join("apps/main/src/bin")).unwrap();
        fs::write(
            tmp.path().join("apps/main/src/bin/commands.rs"),
            "fn main() {}",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("apps/worker/src")).unwrap();
        fs::create_dir_all(tmp.path().join("apps/billing/src/bin")).unwrap();
        fs::write(
            tmp.path().join("apps/billing/src/bin/commands.rs"),
            "fn main() {}",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("apps/ledger/src/bin/commands")).unwrap();
        fs::write(
            tmp.path().join("apps/ledger/src/bin/commands/main.rs"),
            "fn main() {}",
        )
        .unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert_eq!(
            project.apps_with_commands(),
            vec!["billing", "ledger", "main"]
        );
    }

    #[test]
    fn apps_with_commands_is_empty_without_an_apps_dir() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MARKER), "[project]\n").unwrap();
        let project = Project::discover_from(tmp.path()).unwrap();
        assert!(project.apps_with_commands().is_empty());
    }
}
