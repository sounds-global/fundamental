//! The packages a workspace builds from this machine, and which a change reaches.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::CliResult;
use crate::helpers::git;

use super::changed_files::changed_since_trunk;
use super::local_package::LocalPackage;
use super::selection::Selection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Workspace {
    pub(super) root: PathBuf,
    pub(super) packages: Vec<LocalPackage>,
}

impl Workspace {
    /// Read from `cargo metadata`; only path packages count, since a registry crate changes with the lockfile.
    pub(crate) fn from_metadata(metadata: &Value) -> Option<Self> {
        let members: Vec<&str> = metadata["workspace_members"]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let local: Vec<&Value> = metadata["packages"]
            .as_array()?
            .iter()
            .filter(|package| package["source"].is_null())
            .collect();
        let name_of = |id: &str| {
            local
                .iter()
                .find(|package| package["id"].as_str() == Some(id))
                .and_then(|package| package["name"].as_str())
        };
        let nodes = metadata["resolve"]["nodes"].as_array()?;
        let packages = local
            .iter()
            .map(|package| {
                let id = package["id"].as_str()?;
                let depends = nodes
                    .iter()
                    .find(|node| node["id"].as_str() == Some(id))
                    .and_then(|node| node["dependencies"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(name_of)
                    .map(str::to_owned)
                    .collect();
                let reads = package["metadata"]["fun"]["reads"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(LocalPackage {
                    name: package["name"].as_str()?.to_owned(),
                    dir: Path::new(package["manifest_path"].as_str()?)
                        .parent()?
                        .to_path_buf(),
                    member: members.contains(&id),
                    depends,
                    reads,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            root: PathBuf::from(metadata["workspace_root"].as_str()?),
            packages,
        })
    }

    /// The members the change since the trunk reaches, across every repository a path package lives in.
    pub(crate) fn affected(&self) -> CliResult<Selection> {
        let mut toplevels: Vec<PathBuf> = Vec::new();
        for dir in
            std::iter::once(&self.root).chain(self.packages.iter().map(|package| &package.dir))
        {
            if toplevels.iter().any(|toplevel| dir.starts_with(toplevel)) || !dir.is_dir() {
                continue;
            }
            toplevels.push(PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?));
        }
        let mut changed = Vec::new();
        for toplevel in &toplevels {
            changed.extend(changed_since_trunk(toplevel)?);
        }
        Ok(self.select(&changed))
    }

    /// The members a change to `changed` reaches: owners, their dependents, and packages whose `reads` cover it.
    ///
    /// An unread workspace file outside every package, or a build-wide file, reaches everything.
    pub(super) fn select(&self, changed: &[PathBuf]) -> Selection {
        let mut reached: BTreeSet<&str> = BTreeSet::new();
        let mut read_by: BTreeSet<&str> = BTreeSet::new();
        for path in changed {
            let relative = path
                .strip_prefix(&self.root)
                .ok()
                .map(|relative| relative.to_string_lossy().into_owned());
            if relative.as_deref().is_some_and(builds_everything) {
                return Selection::Everything;
            }
            let readers = relative
                .as_deref()
                .map(|relative| self.readers_of(relative))
                .unwrap_or_default();
            let owner = self.owner_of(path);
            if owner.is_none() && readers.is_empty() && relative.is_some() {
                return Selection::Everything;
            }
            reached.extend(owner.map(|owner| owner.name.as_str()));
            read_by.extend(readers);
        }
        let mut selected = self.dependents_of(reached);
        selected.extend(
            self.packages
                .iter()
                .filter(|package| package.member && read_by.contains(package.name.as_str()))
                .map(|package| package.name.clone()),
        );
        Selection::Packages(selected)
    }

    fn readers_of(&self, relative: &str) -> Vec<&str> {
        self.packages
            .iter()
            .filter(|package| {
                package
                    .reads
                    .iter()
                    .any(|glob| super::reads_cover::reads_cover(glob, relative))
            })
            .map(|package| package.name.as_str())
            .collect()
    }

    /// The innermost package whose directory holds `path`.
    fn owner_of(&self, path: &Path) -> Option<&LocalPackage> {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.dir))
            .max_by_key(|package| package.dir.components().count())
    }

    fn dependents_of(&self, reached: BTreeSet<&str>) -> BTreeSet<String> {
        let mut found = reached;
        loop {
            let more: Vec<&str> = self
                .packages
                .iter()
                .filter(|package| !found.contains(package.name.as_str()))
                .filter(|package| {
                    package
                        .depends
                        .iter()
                        .any(|dep| found.contains(dep.as_str()))
                })
                .map(|package| package.name.as_str())
                .collect();
            if more.is_empty() {
                break;
            }
            found.extend(more);
        }
        self.packages
            .iter()
            .filter(|package| package.member && found.contains(package.name.as_str()))
            .map(|package| package.name.clone())
            .collect()
    }
}

/// The workspace manifest and lockfile, the toolchain, and cargo's and nextest's configuration.
fn builds_everything(relative: &str) -> bool {
    matches!(
        relative,
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml"
    ) || relative.starts_with(".cargo/")
        || relative.starts_with(".config/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, dir: &str, member: bool, depends: &[&str]) -> LocalPackage {
        LocalPackage {
            name: name.to_owned(),
            dir: PathBuf::from(dir),
            member,
            depends: depends.iter().map(|dep| (*dep).to_owned()).collect(),
            reads: Vec::new(),
        }
    }

    fn workspace() -> Workspace {
        let mut gates = package("gates", "/p/gates", true, &[]);
        gates.reads = vec!["docs/**".to_owned()];
        Workspace {
            root: PathBuf::from("/p"),
            packages: vec![
                package("app", "/p/apps/app", true, &["domain"]),
                package("domain", "/p/domain", true, &["core"]),
                package("other", "/p/other", true, &[]),
                gates,
                package("core", "/f/crates/core", false, &[]),
            ],
        }
    }

    fn names(names: &[&str]) -> Selection {
        Selection::Packages(names.iter().map(|name| (*name).to_owned()).collect())
    }

    #[test]
    fn the_workspace_is_read_from_cargo_metadata_with_its_path_packages_only() {
        let metadata = serde_json::json!({
            "workspace_root": "/p",
            "workspace_members": ["app 0.1.0 (path+file:///p/app)", "gates 0.1.0 (path+file:///p/gates)"],
            "packages": [
                {"id": "app 0.1.0 (path+file:///p/app)", "name": "app", "source": null,
                 "manifest_path": "/p/app/Cargo.toml", "metadata": null},
                {"id": "gates 0.1.0 (path+file:///p/gates)", "name": "gates", "source": null,
                 "manifest_path": "/p/gates/Cargo.toml",
                 "metadata": {"fun": {"reads": ["docs/**"]}}},
                {"id": "core 0.1.0 (path+file:///f/core)", "name": "core", "source": null,
                 "manifest_path": "/f/core/Cargo.toml", "metadata": null},
                {"id": "serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)",
                 "name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index",
                 "manifest_path": "/r/serde/Cargo.toml", "metadata": null}
            ],
            "resolve": {"nodes": [
                {"id": "app 0.1.0 (path+file:///p/app)", "dependencies": [
                    "core 0.1.0 (path+file:///f/core)",
                    "serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)"]},
                {"id": "gates 0.1.0 (path+file:///p/gates)", "dependencies": []},
                {"id": "core 0.1.0 (path+file:///f/core)", "dependencies": []}
            ]}
        });
        let read = Workspace::from_metadata(&metadata).expect("the metadata reads");
        assert_eq!(read.root, PathBuf::from("/p"));
        assert_eq!(
            read.packages,
            vec![
                package("app", "/p/app", true, &["core"]),
                LocalPackage {
                    reads: vec!["docs/**".to_owned()],
                    ..package("gates", "/p/gates", true, &[])
                },
                package("core", "/f/core", false, &[]),
            ]
        );
    }

    #[test]
    fn a_change_in_a_package_runs_it_and_every_member_that_depends_on_it() {
        let selected = workspace().select(&[PathBuf::from("/p/domain/src/lib.rs")]);
        assert_eq!(selected, names(&["app", "domain"]));
    }

    #[test]
    fn a_change_in_a_path_dependency_runs_the_members_that_reach_it() {
        let selected = workspace().select(&[PathBuf::from("/f/crates/core/src/lib.rs")]);
        assert_eq!(selected, names(&["app", "domain"]), "core is no member");
    }

    #[test]
    fn a_file_a_package_declares_it_reads_runs_that_package() {
        let selected = workspace().select(&[PathBuf::from("/p/docs/guide.md")]);
        assert_eq!(selected, names(&["gates"]));
    }

    #[test]
    fn a_file_only_a_packages_tests_read_never_runs_the_packages_depending_on_it() {
        let mut reading = workspace();
        reading.packages[2].depends.push("gates".to_owned());
        let selected = reading.select(&[PathBuf::from("/p/docs/guide.md")]);
        assert_eq!(selected, names(&["gates"]));
    }

    #[test]
    fn a_workspace_file_no_package_reads_runs_everything() {
        let selected = workspace().select(&[
            PathBuf::from("/p/other/src/lib.rs"),
            PathBuf::from("/p/Cargo.lock"),
        ]);
        assert_eq!(selected, Selection::Everything);
    }

    #[test]
    fn a_file_in_a_package_also_runs_a_package_that_reads_it() {
        let mut reading = workspace();
        reading.packages[3].reads.push("other/**".to_owned());
        let selected = reading.select(&[PathBuf::from("/p/other/styles/theme.css")]);
        assert_eq!(selected, names(&["gates", "other"]));
    }

    #[test]
    fn a_change_to_how_everything_builds_runs_everything_whoever_reads_it() {
        let mut reading = workspace();
        reading.packages[3].reads.push("**".to_owned());
        for file in [
            "/p/Cargo.lock",
            "/p/Cargo.toml",
            "/p/rust-toolchain.toml",
            "/p/.config/nextest.toml",
        ] {
            assert_eq!(
                reading.select(&[PathBuf::from(file)]),
                Selection::Everything,
                "{file}"
            );
        }
    }

    #[test]
    fn a_peer_file_in_no_package_reaches_nothing() {
        let selected = workspace().select(&[PathBuf::from("/f/docs/book.md")]);
        assert_eq!(selected, names(&[]));
    }

    #[test]
    fn a_nested_package_owns_its_own_files() {
        let mut nested = workspace();
        nested
            .packages
            .push(package("inner", "/p/apps/app/inner", true, &[]));
        let selected = nested.select(&[PathBuf::from("/p/apps/app/inner/src/lib.rs")]);
        assert_eq!(selected, names(&["inner"]));
    }
}
