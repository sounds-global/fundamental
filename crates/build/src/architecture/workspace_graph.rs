//! [`WorkspaceGraph`], the workspace's crates and the edges between them, read without cargo.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::IgnoredAny;
use toml::Spanned;
use tracing::{debug, instrument};

use crate::build_error::BuildError;

use super::architecture_finding::ArchitectureFinding;
use super::dependency::Dependency;
use super::member::Member;

/// The workspace's crates and what each depends on, read from their `Cargo.toml` files
/// without cargo, which a build script must not start.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceGraph {
    /// Every member: the declared ones in the root's order, then those path dependencies add.
    pub(crate) members: Vec<Member>,
    /// Every `Cargo.toml` read, absolute.
    pub(crate) inputs: Vec<PathBuf>,
}

impl WorkspaceGraph {
    /// Read the workspace rooted at `root`; a member pattern this reader cannot expand is
    /// reported in `findings` rather than guessed.
    ///
    /// # Errors
    ///
    /// [`BuildError::Io`] or [`BuildError::Parse`] when a `Cargo.toml` cannot be read.
    #[instrument(skip_all, fields(root = %root.display()), err(Debug))]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    pub(crate) fn read(
        root: &Path,
        findings: &mut Vec<ArchitectureFinding>,
    ) -> Result<Self, BuildError> {
        let mut graph = Self::default();
        let root_manifest = graph.parse(root, Path::new(""))?;
        let canonical_root = fs::canonicalize(root).map_err(|e| BuildError::io(root, e))?;
        let empty = WorkspaceSection::default();
        let workspace = root_manifest.workspace.as_ref().unwrap_or(&empty);
        let workspace_packages: BTreeMap<String, String> = workspace
            .dependencies
            .iter()
            .map(|(key, dep)| (key.get_ref().clone(), dep.package_or(key.get_ref())))
            .collect();
        let workspace_paths: BTreeMap<String, PathBuf> = workspace
            .dependencies
            .iter()
            .filter_map(|(key, dep)| {
                dep.path()
                    .map(|path| (key.get_ref().clone(), root.join(path)))
            })
            .collect();
        let excluded = |dir: &Path| {
            workspace
                .exclude
                .iter()
                .any(|excluded| Path::new(excluded.trim_end_matches('/')) == dir)
        };

        let mut queue: VecDeque<PathBuf> = VecDeque::new();
        if root_manifest.package.is_some() {
            queue.push_back(PathBuf::new());
        }
        for pattern in &workspace.members {
            match expand(root, pattern.get_ref()) {
                Ok(found) => queue.extend(found),
                Err(reason) => findings.push(ArchitectureFinding::InvalidTable {
                    at: format!("Cargo.toml:{}", line_of(&root_manifest.source, pattern)),
                    reason,
                }),
            }
        }

        // Cargo makes every path dependency inside the workspace directory a member too.
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        while let Some(dir) = queue.pop_front() {
            if excluded(&dir) || !seen.insert(dir.clone()) {
                continue;
            }
            let manifest = if dir.as_os_str().is_empty() {
                root_manifest.clone()
            } else {
                graph.parse(root, &dir)?
            };
            for target in manifest.path_dependencies(&root.join(&dir), &workspace_paths) {
                match fs::canonicalize(&target) {
                    Ok(target) => {
                        if let Ok(inside) = target.strip_prefix(&canonical_root)
                            && target.join("Cargo.toml").is_file()
                        {
                            queue.push_back(inside.to_path_buf());
                        }
                    }
                    Err(error) => debug!(
                        path = %target.display(),
                        %error,
                        "a path dependency that does not resolve is cargo's to refuse"
                    ),
                }
            }
            let Some(package) = manifest.package.as_ref() else {
                continue;
            };
            let ident = manifest
                .lib
                .as_ref()
                .and_then(|lib| lib.name.clone())
                .unwrap_or_else(|| package.name.replace('-', "_"));
            let dependencies = manifest.dependencies(&workspace_packages);
            graph.members.push(Member {
                package: package.name.clone(),
                ident,
                dir,
                dependencies,
            });
        }
        graph.use_library_names();
        debug!(members = graph.members.len(), "read the workspace");
        Ok(graph)
    }

    /// The member whose directory, relative to the project, is `dir`.
    pub(crate) fn member_at(&self, dir: &Path) -> Option<&Member> {
        self.members.iter().find(|member| member.dir == dir)
    }

    fn parse(&mut self, root: &Path, dir: &Path) -> Result<CrateManifest, BuildError> {
        let path = root.join(dir).join("Cargo.toml");
        let source = fs::read_to_string(&path).map_err(|e| BuildError::io(&path, e))?;
        let mut manifest: CrateManifest =
            toml::from_str(&source).map_err(|e| BuildError::Parse {
                file: path.clone(),
                message: e.message().to_owned(),
            })?;
        manifest.source = source;
        self.inputs.push(path);
        Ok(manifest)
    }

    /// A member a dependency names by its package, unrenamed, is named in source by its `[lib] name`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn use_library_names(&mut self) {
        let idents: BTreeMap<String, String> = self
            .members
            .iter()
            .map(|member| (member.package.clone(), member.ident.clone()))
            .collect();
        for member in &mut self.members {
            for dependency in &mut member.dependencies {
                if dependency.ident == dependency.package.replace('-', "_")
                    && let Some(ident) = idents.get(&dependency.package)
                {
                    dependency.ident.clone_from(ident);
                }
            }
        }
    }
}

/// The directories one `members` pattern names, relative to `root`; only a literal path or a
/// trailing `*` is expanded.
fn expand(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    let is_glob = |segment: &str| segment.contains(['*', '?', '[', ']', '{', '}']);
    let pattern = pattern.trim_end_matches('/');
    let (parent, last) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    if is_glob(parent) || (is_glob(last) && last != "*") {
        return Err(format!(
            "the workspace member pattern `{pattern}` cannot be read without cargo; write a literal path or a final `*` segment"
        ));
    }
    if last != "*" {
        return Ok(vec![PathBuf::from(pattern)]);
    }
    let base = root.join(parent);
    let unreadable = |e: std::io::Error| format!("the workspace member pattern `{pattern}`: {e}");
    let mut dirs = Vec::new();
    for entry in fs::read_dir(&base).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        if entry.path().join("Cargo.toml").is_file() {
            dirs.push(Path::new(parent).join(entry.file_name()));
        }
    }
    dirs.sort();
    Ok(dirs)
}

pub(crate) fn line_of<T>(source: &str, spanned: &Spanned<T>) -> usize {
    source
        .get(..spanned.span().start)
        .map_or(1, |before| before.matches('\n').count() + 1)
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CrateManifest {
    #[serde(skip)]
    source: String,
    package: Option<PackageSection>,
    lib: Option<LibSection>,
    workspace: Option<WorkspaceSection>,
    #[serde(default)]
    dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
    #[serde(default, alias = "build_dependencies", rename = "build-dependencies")]
    build_dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
    #[serde(default, alias = "dev_dependencies", rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
    #[serde(default)]
    target: BTreeMap<String, TargetSection>,
}

impl CrateManifest {
    /// Every dependency's path, absolute, development dependencies included.
    fn path_dependencies(
        &self,
        dir: &Path,
        workspace_paths: &BTreeMap<String, PathBuf>,
    ) -> Vec<PathBuf> {
        self.all_entries()
            .filter_map(|(key, entry)| match entry {
                DependencyEntry::Detailed {
                    path: Some(path), ..
                } => Some(dir.join(path)),
                DependencyEntry::Detailed {
                    workspace: true, ..
                } => workspace_paths.get(key.get_ref()).cloned(),
                DependencyEntry::Detailed { .. } | DependencyEntry::Version(_) => None,
            })
            .collect()
    }

    fn all_entries(&self) -> impl Iterator<Item = (&Spanned<String>, &DependencyEntry)> {
        self.dependencies
            .iter()
            .chain(&self.build_dependencies)
            .chain(&self.dev_dependencies)
            .chain(self.target.values().flat_map(|target| {
                target
                    .dependencies
                    .iter()
                    .chain(&target.build_dependencies)
                    .chain(&target.dev_dependencies)
            }))
    }

    fn dependencies(&self, workspace_packages: &BTreeMap<String, String>) -> Vec<Dependency> {
        let normal =
            self.dependencies
                .iter()
                .chain(&self.build_dependencies)
                .chain(self.target.values().flat_map(|target| {
                    target.dependencies.iter().chain(&target.build_dependencies)
                }))
                .map(|entry| (entry, false));
        let dev = self
            .dev_dependencies
            .iter()
            .chain(
                self.target
                    .values()
                    .flat_map(|target| &target.dev_dependencies),
            )
            .map(|entry| (entry, true));
        normal
            .chain(dev)
            .map(|((key, entry), dev)| Dependency {
                ident: key.get_ref().replace('-', "_"),
                package: entry.resolve(key.get_ref(), workspace_packages),
                line: line_of(&self.source, key),
                dev,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PackageSection {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LibSection {
    name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkspaceSection {
    #[serde(default)]
    members: Vec<Spanned<String>>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TargetSection {
    #[serde(default)]
    dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
    #[serde(default, alias = "build_dependencies", rename = "build-dependencies")]
    build_dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
    #[serde(default, alias = "dev_dependencies", rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<Spanned<String>, DependencyEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DependencyEntry {
    Detailed {
        package: Option<String>,
        path: Option<String>,
        #[serde(default)]
        workspace: bool,
    },
    Version(IgnoredAny),
}

impl DependencyEntry {
    fn package_or(&self, key: &str) -> String {
        match self {
            Self::Detailed {
                package: Some(package),
                ..
            } => package.clone(),
            Self::Version(_) | Self::Detailed { .. } => key.to_owned(),
        }
    }

    fn path(&self) -> Option<&str> {
        match self {
            Self::Detailed { path, .. } => path.as_deref(),
            Self::Version(_) => None,
        }
    }

    fn resolve(&self, key: &str, workspace_packages: &BTreeMap<String, String>) -> String {
        match self {
            Self::Detailed {
                package: None,
                workspace: true,
                ..
            } => workspace_packages
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.to_owned()),
            _ => self.package_or(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn package(name: &str, deps: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n{deps}")
    }

    fn read(root: &Path) -> (WorkspaceGraph, Vec<ArchitectureFinding>) {
        let mut findings = Vec::new();
        let graph = WorkspaceGraph::read(root, &mut findings).unwrap();
        (graph, findings)
    }

    #[test]
    fn members_come_from_literal_paths_and_a_final_star() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"domain/*\", \"legal\"]\nexclude = [\"domain/old\"]\n",
        );
        write(d.path(), "domain/b/Cargo.toml", &package("domain_b", ""));
        write(d.path(), "domain/a/Cargo.toml", &package("domain_a", ""));
        write(d.path(), "domain/old/Cargo.toml", &package("old", ""));
        fs::create_dir_all(d.path().join("domain/not_a_crate")).unwrap();
        write(d.path(), "legal/Cargo.toml", &package("legal-texts", ""));
        let (graph, findings) = read(d.path());
        assert!(findings.is_empty());
        let names: Vec<_> = graph.members.iter().map(|m| m.package.as_str()).collect();
        assert_eq!(names, ["domain_a", "domain_b", "legal-texts"]);
        assert_eq!(graph.members[2].ident, "legal_texts");
        assert_eq!(graph.inputs.len(), 4);
    }

    #[test]
    fn a_pattern_cargo_alone_could_expand_is_named_not_guessed() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\n  \"crates/*/sub\",\n]\n",
        );
        let (graph, findings) = read(d.path());
        assert!(graph.members.is_empty());
        assert!(matches!(
            &findings[..],
            [ArchitectureFinding::InvalidTable { at, reason }]
                if at == "Cargo.toml:3" && reason.contains("crates/*/sub")
        ));
    }

    #[test]
    fn a_pattern_over_a_directory_that_cannot_be_read_is_named() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"gone/*\"]\n",
        );
        let (graph, findings) = read(d.path());
        assert!(graph.members.is_empty());
        assert!(matches!(
            &findings[..],
            [ArchitectureFinding::InvalidTable { at, reason }]
                if at == "Cargo.toml:2" && reason.contains("`gone/*`")
        ));
    }

    #[test]
    fn renames_and_workspace_dependencies_resolve_to_their_package() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"store\"]\n\n[workspace.dependencies]\nshop = { path = \"store\", package = \"store_impl\" }\n",
        );
        write(
            d.path(),
            "store/Cargo.toml",
            "[package]\nname = \"store_impl\"\nversion = \"0.1.0\"\n\n[lib]\nname = \"store\"\n",
        );
        write(
            d.path(),
            "app/Cargo.toml",
            &package(
                "app",
                "[dependencies]\nshop = { workspace = true }\nmy-db = { version = \"1\", package = \"real-db\" }\nstore_impl = { path = \"../store\" }\n\n[target.'cfg(unix)'.build-dependencies]\nbuilder = \"1\"\n\n[dev-dependencies]\ntesting = \"1\"\n",
            ),
        );
        let (graph, _) = read(d.path());
        let app = graph.member_at(Path::new("app")).unwrap();
        let found: Vec<_> = app
            .dependencies
            .iter()
            .map(|d| (d.ident.as_str(), d.package.as_str(), d.line, d.dev))
            .collect();
        assert!(found.contains(&("shop", "store_impl", 6, false)));
        assert!(found.contains(&("my_db", "real-db", 7, false)));
        assert!(found.contains(&("store", "store_impl", 8, false)));
        assert!(found.contains(&("builder", "builder", 11, false)));
        assert!(found.contains(&("testing", "testing", 14, true)));
    }

    #[test]
    fn a_path_dependency_inside_the_workspace_is_a_member_as_cargo_has_it() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"apps/*\", \"apps/web\"]\nexclude = [\"vendor/skip\"]\n\n[workspace.dependencies]\nshared = { path = \"crates/shared\" }\n",
        );
        write(
            d.path(),
            "apps/web/Cargo.toml",
            &package(
                "web",
                "[dependencies]\nstore = { path = \"../../crates/store\" }\nshared = { workspace = true }\nskip = { path = \"../../vendor/skip\" }\n",
            ),
        );
        write(
            d.path(),
            "crates/store/Cargo.toml",
            &package(
                "store",
                "[dev-dependencies]\nfixtures = { path = \"../fixtures\" }\n",
            ),
        );
        write(d.path(), "crates/shared/Cargo.toml", &package("shared", ""));
        write(
            d.path(),
            "crates/fixtures/Cargo.toml",
            &package("fixtures", ""),
        );
        write(d.path(), "vendor/skip/Cargo.toml", &package("skip", ""));
        let (graph, findings) = read(d.path());
        assert!(findings.is_empty());
        let mut names: Vec<_> = graph.members.iter().map(|m| m.package.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["fixtures", "shared", "store", "web"]);
    }

    #[test]
    fn a_root_package_is_a_member() {
        let d = TempDir::new().unwrap();
        write(d.path(), "Cargo.toml", &package("solo", ""));
        let (graph, _) = read(d.path());
        assert_eq!(graph.members.len(), 1);
        assert_eq!(graph.members[0].dir, PathBuf::new());
    }

    #[test]
    fn an_unreadable_member_manifest_is_a_parse_error() {
        let d = TempDir::new().unwrap();
        write(d.path(), "Cargo.toml", "[workspace]\nmembers = [\"a\"]\n");
        write(d.path(), "a/Cargo.toml", "[package\n");
        let err = WorkspaceGraph::read(d.path(), &mut Vec::new()).unwrap_err();
        assert!(matches!(err, BuildError::Parse { .. }));
    }
}
