//! [`ArchitectureCheck`], the dependency rule judged over one app and its workspace.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fundamental_core::AnyError;
use fundamental_manifest::pending::Unheld;
use fundamental_manifest::{Manifest, ManifestError};
use tracing::{debug, info, instrument, warn};

use crate::build_error::BuildError;
use crate::route::RouteTree;

use super::architecture_finding::ArchitectureFinding;
use super::architecture_table::ArchitectureTable;
use super::file_rule::FileRule;
use super::member::Member;
use super::verdict::Verdict;
use super::view_scan::{Forbidden, scan_view};
use super::workspace_graph::WorkspaceGraph;

/// Page source checks and optional dependency rules, judged over one app and its workspace.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArchitectureCheck<'a> {
    /// The app's directory, absolute.
    pub(crate) app_dir: &'a Path,
    /// The app's `pages/` directory, absolute.
    pub(crate) pages_dir: &'a Path,
    /// The scanned pages.
    pub(crate) tree: &'a RouteTree,
}

/// A finding of a file rule, with the file it is about, so a pending entry can claim it.
struct FileFinding {
    file: PathBuf,
    rule: FileRule,
    finding: ArchitectureFinding,
}

impl FileFinding {
    /// The crate a `BUILD_029` finding reaches, which its pending entry must name.
    fn name(&self) -> Option<String> {
        match &self.finding {
            ArchitectureFinding::AdapterInView { krate, .. }
            | ArchitectureFinding::OutwardEdge { to: krate, .. } => Some(krate.clone()),
            _ => None,
        }
    }
}

impl ArchitectureCheck<'_> {
    /// Judge the app and its workspace, adding every file and directory read to `inputs`
    /// whatever the verdict.
    ///
    /// # Errors
    ///
    /// [`BuildError::ArchitectureRefused`] with every finding outside the pending list;
    /// [`BuildError::Io`] or [`BuildError::Parse`] when a file cannot be read;
    /// [`BuildError::Other`] when the manifest cannot be loaded.
    #[instrument(skip_all, fields(app_dir = %self.app_dir.display()), err(Debug))]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "readability baseline"
    )]
    pub(crate) fn run(&self, inputs: &mut Vec<PathBuf>) -> Result<Verdict, BuildError> {
        let workspace = workspace_root(self.app_dir)?;
        let mut root = match Manifest::find_root(self.app_dir) {
            Ok(root) => root,
            Err(ManifestError::NotFound { .. }) => workspace.clone(),
            Err(error) => return Err(BuildError::Other(AnyError::from(error))),
        };
        // A manifest nearer the app than the workspace root is another project's.
        if root != workspace {
            if workspace.join("fun.toml").is_file() {
                return Err(BuildError::architecture_refused(vec![
                    ArchitectureFinding::InvalidTable {
                        at: format!("{}:1", root.join("fun.toml").display()),
                        reason: format!(
                            "this manifest stands between the app and the project's own at {}; the project has one manifest, at its workspace root",
                            workspace.join("fun.toml").display()
                        ),
                    },
                ]));
            }
            root = workspace.clone();
        }
        let manifest = if root.join("fun.toml").is_file() {
            Some(Manifest::load(&root).map_err(|e| BuildError::Other(AnyError::from(e)))?)
        } else {
            None
        };
        if let Some(manifest) = &manifest {
            inputs.extend(
                manifest
                    .files()
                    .iter()
                    .map(|file| manifest.source_dir().join(file)),
            );
        } else {
            inputs.push(root.join("fun.toml"));
        }
        let mut findings = Vec::new();
        let table = manifest
            .as_ref()
            .map_or_else(ArchitectureTable::default, |manifest| {
                ArchitectureTable::read(manifest, &mut findings)
            });
        if table.unreadable {
            return Err(BuildError::architecture_refused(findings));
        }
        let graph = WorkspaceGraph::read(&root, &mut findings)?;
        inputs.extend(graph.inputs.iter().cloned());
        let lock = root.join("Cargo.lock");
        if lock.is_file() {
            inputs.push(lock);
        }

        let mut file_findings = judge_the_crates(&table, &graph, &mut findings);

        let app = relative(&root, self.app_dir);
        let app_src = self.app_dir.join("src");
        let forbidden = forbidden_to_views(&table, graph.member_at(&app));
        if app_src.is_dir() {
            inputs.push(app_src.clone());
        }
        let views = self.view_files(&app_src)?;
        for view in &views {
            let source = fs::read_to_string(view).map_err(|e| BuildError::io(view, e))?;
            let parsed = syn::parse_file(&source).map_err(|e| BuildError::Parse {
                file: view.clone(),
                message: e.to_string(),
            })?;
            let file = relative(&root, view);
            for (rule, finding) in scan_view(&parsed, &file, &forbidden) {
                file_findings.push(FileFinding {
                    file: file.clone(),
                    rule,
                    finding,
                });
            }
        }
        if !self.tree.serves_page_state() {
            for (route, directory) in self.tree.routes_beside_their_directories(self.pages_dir) {
                let file = relative(&root, &self.pages_dir.join(route));
                let directory = relative(&root, &self.pages_dir.join(directory));
                file_findings.push(FileFinding {
                    finding: ArchitectureFinding::RouteBesideItsDirectory {
                        at: format!("{}:1", file.display()),
                        directory: directory.display().to_string(),
                    },
                    file,
                    rule: FileRule::RouteBesideItsDirectory,
                });
            }
        }
        let frozen = hold_to_pending(&table, &root, &app, file_findings, &mut findings);
        debug!(
            views = views.len(),
            members = graph.members.len(),
            rules = table.forbidden.len(),
            pending = table.pending.entries().len(),
            frozen,
            findings = findings.len(),
            "judged the dependency rule"
        );
        if findings.is_empty() {
            info!(views = views.len(), frozen, "page checks passed");
            Ok(Verdict::Passed)
        } else {
            warn!(findings = findings.len(), "page checks refused the build");
            Err(BuildError::architecture_refused(findings))
        }
    }

    /// Every page, layout and root special file, and every `present.rs` under the app's `src/`.
    fn view_files(&self, app_src: &Path) -> Result<Vec<PathBuf>, BuildError> {
        let tree = self.tree;
        let mut views: Vec<PathBuf> = tree
            .routes
            .iter()
            .map(|route| &route.module_path)
            .chain(tree.layouts.iter().map(|layout| &layout.file_path))
            .chain(&tree.document)
            .chain(&tree.not_found)
            .chain(&tree.error_page)
            .chain(&tree.gate)
            .map(|file| self.pages_dir.join(file))
            .collect();
        presenters(app_src, &mut views)?;
        views.sort();
        views.dedup();
        Ok(views)
    }
}

/// Refuses a rule for a crate the workspace lacks; returns each forbidden dependency edge.
fn judge_the_crates(
    table: &ArchitectureTable,
    graph: &WorkspaceGraph,
    findings: &mut Vec<ArchitectureFinding>,
) -> Vec<FileFinding> {
    let mut edges = Vec::new();
    // Only a rule naming no crate of the workspace is a finding here.
    for source in table.forbidden.keys() {
        if !graph.members.iter().any(|member| member.package == *source) {
            findings.push(ArchitectureFinding::InvalidTable {
                at: table.at.clone(),
                reason: format!("`{source}` is not a workspace crate named by any dependency rule"),
            });
        }
    }
    for member in &graph.members {
        for dependency in member.dependencies.iter().filter(|d| !d.dev) {
            if table
                .forbidden
                .get(&member.package)
                .is_some_and(|names| names.contains(&dependency.package))
            {
                edges.push(FileFinding {
                    file: member.manifest(),
                    rule: FileRule::OutwardEdge,
                    finding: ArchitectureFinding::OutwardEdge {
                        at: format!("{}:{}", member.manifest().display(), dependency.line),
                        from: member.package.clone(),
                        from_ring: "crate".to_owned(),
                        to: dependency.package.clone(),
                        to_ring: "forbidden dependency".to_owned(),
                    },
                });
            }
        }
    }
    edges
}

/// The crates the app depends on that its views may not name, keyed by the name source uses.
fn forbidden_to_views(
    table: &ArchitectureTable,
    app: Option<&Member>,
) -> BTreeMap<String, Forbidden> {
    let Some(app) = app else {
        return BTreeMap::new();
    };
    table
        .forbidden
        .get(&app.package)
        .into_iter()
        .flatten()
        .map(|package| {
            let ident = package.replace('-', "_");
            (
                ident.clone(),
                Forbidden {
                    ident,
                    ring: "forbidden dependency".to_owned(),
                },
            )
        })
        .collect()
}

/// Hold each file finding against the pending list; returns how many findings it holds.
fn hold_to_pending(
    table: &ArchitectureTable,
    root: &Path,
    app: &Path,
    file_findings: Vec<FileFinding>,
    findings: &mut Vec<ArchitectureFinding>,
) -> usize {
    let mut by_file: BTreeMap<(PathBuf, FileRule, Option<String>), Vec<ArchitectureFinding>> =
        BTreeMap::new();
    for found in file_findings {
        let name = found.name();
        by_file
            .entry((found.file, found.rule, name))
            .or_default()
            .push(found.finding);
    }
    // A view belongs to one app's build; the crate graph is judged whole in every build.
    let held = table.pending.hold(root, by_file, |entry| {
        entry.rule == FileRule::OutwardEdge || entry.file.starts_with(app)
    });
    findings.extend(held.unheld.into_iter().map(|unheld| match unheld {
        Unheld::Mismatch(mismatch) => ArchitectureFinding::PendingMismatch {
            at: mismatch.at,
            file: mismatch.file,
            rule: mismatch.rule.to_owned(),
            frozen: mismatch.frozen,
            found: mismatch.found,
            advice: mismatch.advice,
        },
        Unheld::Finding(finding) => finding,
    }));
    held.frozen
}

/// The nearest directory at or above `app_dir` whose `Cargo.toml` declares `[workspace]`,
/// or `app_dir` itself.
fn workspace_root(app_dir: &Path) -> Result<PathBuf, BuildError> {
    for directory in app_dir.ancestors() {
        let manifest = directory.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let source = fs::read_to_string(&manifest).map_err(|e| BuildError::io(&manifest, e))?;
        let table: toml::Table = toml::from_str(&source).map_err(|e| BuildError::Parse {
            file: manifest.clone(),
            message: e.message().to_owned(),
        })?;
        if table.contains_key("workspace") {
            return Ok(directory.to_path_buf());
        }
    }
    Ok(app_dir.to_path_buf())
}

fn presenters(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), BuildError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| BuildError::io(dir, e))? {
        let entry = entry.map_err(|e| BuildError::io(dir, e))?;
        let kind = entry
            .file_type()
            .map_err(|e| BuildError::io(entry.path(), e))?;
        let path = entry.path();
        if kind.is_dir() {
            presenters(&path, found)?;
        } else if kind.is_file() && entry.file_name() == "present.rs" {
            found.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    let resolve = |path: &Path| {
        fs::canonicalize(path).unwrap_or_else(|error| {
            debug!(path = %path.display(), %error, "kept a path unresolved");
            path.to_path_buf()
        })
    };
    let (root, path) = (resolve(root), resolve(path));
    path.strip_prefix(&root)
        .map_or_else(|_| path.clone(), Path::to_path_buf)
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

    fn judge(root: &Path) -> (Result<Verdict, BuildError>, Vec<PathBuf>) {
        let app = fs::canonicalize(root.join("app")).unwrap();
        let pages = app.join("pages");
        let tree = RouteTree::scan(&pages).unwrap();
        let mut inputs = Vec::new();
        let verdict = ArchitectureCheck {
            app_dir: &app,
            pages_dir: &pages,
            tree: &tree,
        }
        .run(&mut inputs);
        (verdict, inputs)
    }

    fn project(table: &str) -> TempDir {
        let d = TempDir::new().unwrap();
        let root = d.path();
        write(
            root,
            "fun.toml",
            &format!("[project]\nname = \"demo\"\n\n{table}"),
        );
        write(root, "Cargo.toml", "[workspace]\nmembers = [\"app\"]\n");
        write(root, "Cargo.lock", "version = 4\n");
        write(
            root,
            "app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        );
        write(root, "app/src/lib.rs", "");
        write(root, "app/pages/index.rs", "pub fn page() {}");
        d
    }

    #[test]
    fn the_inputs_name_every_file_read() {
        let d = project("[[dependency_rules]]\ncrate = \"app\"\nmust_not_depend_on = []\n");
        let root = fs::canonicalize(d.path()).unwrap();
        let (verdict, inputs) = judge(&root);
        assert_eq!(verdict.unwrap(), Verdict::Passed);
        for expected in [
            "fun.toml",
            "Cargo.toml",
            "app/Cargo.toml",
            "Cargo.lock",
            "app/src",
        ] {
            assert!(
                inputs.contains(&root.join(expected)),
                "{expected} in {inputs:?}"
            );
        }
    }

    #[test]
    fn a_refused_check_still_names_its_inputs() {
        let d =
            project("[[dependency_rules]]\ncrate = \"ghost\"\nmust_not_depend_on = [\"tokio\"]\n");
        let (verdict, inputs) = judge(d.path());
        assert!(matches!(
            verdict,
            Err(BuildError::ArchitectureRefused { .. })
        ));
        assert!(inputs.iter().any(|input| input.ends_with("app/Cargo.toml")));
    }

    #[test]
    fn without_dependency_rules_page_checks_still_run() {
        let d = project("");
        let (verdict, inputs) = judge(d.path());
        assert_eq!(verdict.unwrap(), Verdict::Passed);
        assert!(inputs.iter().any(|input| input.ends_with("fun.toml")));
    }
}
