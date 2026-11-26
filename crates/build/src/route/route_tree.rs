//! [`RouteTree`], the route IR produced by scanning a `pages/` directory.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument};

use crate::build_error::BuildError;

use super::component_name::component_name;
use super::island_resolve::{is_island, page_fn_is_island};
use super::loader_resolve::exports_fn;
use super::page_data_resolve::defines_page_data;
use super::slug_resolve::{declares_slugs, slug_tokens};
use super::special_kind::{SpecialKind, check_reserved};
use super::{Layout, Route, RouteKind};

/// The full route IR for one `pages/` directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RouteTree {
    /// Discovered page routes, sorted static < dynamic < catch-all, then by path.
    pub routes: Vec<Route>,
    /// Discovered `layout.rs` wrappers, outermost first.
    pub layouts: Vec<Layout>,
    /// The `document.rs` HTML shell at the pages root, if present.
    pub document: Option<PathBuf>,
    /// The `not_found.rs` 404 page at the pages root, if present.
    pub not_found: Option<PathBuf>,
    /// The `error.rs` error page at the pages root, if present.
    pub error_page: Option<PathBuf>,
    /// The `gate.rs` page a refused request renders, at the pages root, if present.
    pub gate: Option<PathBuf>,
    /// Every `policy.rs`, in scan order (a directory's before its descendants').
    pub policies: Vec<PathBuf>,
    /// Every `loader.rs`, in scan order.
    pub loaders: Vec<PathBuf>,
}

impl RouteTree {
    /// Scan a pages directory into the route IR; a missing `pages_dir` yields an empty tree.
    ///
    /// # Errors
    /// Returns [`BuildError`] if a directory cannot be read, a file fails to parse, a
    /// reserved or duplicated special file appears, or the tree breaks a page-tree rule.
    #[instrument(skip_all, fields(pages_dir = %pages_dir.display()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn scan(pages_dir: &Path) -> Result<Self, BuildError> {
        let mut tree = RouteTree::default();

        if !pages_dir.is_dir() {
            debug!("pages directory absent; empty route tree");
            return Ok(tree);
        }

        scan_directory(pages_dir, pages_dir, &Descent::root(), &mut tree)?;

        tree.routes.sort_by(|a, b| {
            a.kind
                .priority()
                .cmp(&b.kind.priority())
                .then_with(|| a.url_path.cmp(&b.url_path))
        });

        if let Some(gate) = &tree.gate
            && tree.error_page.is_none()
        {
            return Err(BuildError::GateWithoutErrorPage { file: gate.clone() });
        }
        if tree.serves_page_state() {
            refuse_a_route_beside_its_directory(pages_dir, &tree)?;
            refuse_a_route_outside_its_address_policy(&tree)?;
            refuse_data_under_a_frame(&tree)?;
        }

        debug!(
            routes = tree.routes.len(),
            layouts = tree.layouts.len(),
            policies = tree.policies.len(),
            loaders = tree.loaders.len(),
            has_document = tree.document.is_some(),
            has_not_found = tree.not_found.is_some(),
            has_error_page = tree.error_page.is_some(),
            has_gate = tree.gate.is_some(),
            "scanned pages directory"
        );
        Ok(tree)
    }

    /// Whether the tree serves the app's page state: it has a loader, a policy or a gate.
    #[must_use]
    pub fn serves_page_state(&self) -> bool {
        !self.loaders.is_empty() || !self.policies.is_empty() || self.gate.is_some()
    }

    /// Every page `x.rs` beside a directory `x/`, with that directory, relative to `pages_dir`.
    pub(crate) fn routes_beside_their_directories<'a>(
        &'a self,
        pages_dir: &'a Path,
    ) -> impl Iterator<Item = (&'a Path, PathBuf)> + 'a {
        self.routes.iter().filter_map(move |route| {
            let directory = route.module_path.with_extension("");
            (route
                .module_path
                .file_stem()
                .is_some_and(|stem| stem != "index")
                && pages_dir.join(&directory).is_dir())
            .then_some((route.module_path.as_path(), directory))
        })
    }
}

/// What a directory inherits from the directories above it.
#[derive(Debug, Clone)]
struct Descent {
    /// The URL prefix of the directory, e.g. `/` or `/teams/{team}`.
    url_prefix: String,
    /// The `layout.rs` files wrapping it, outermost first.
    layout_chain: Vec<PathBuf>,
    /// The `policy.rs` files covering it, outermost first.
    policy_chain: Vec<PathBuf>,
    /// The captures of the directories walked so far, outermost first.
    param_chain: Vec<String>,
    /// Whether this directory is a `(group)/`, which contributes no URL segment.
    in_route_group: bool,
}

impl Descent {
    fn root() -> Self {
        Self {
            url_prefix: "/".to_owned(),
            layout_chain: Vec::new(),
            policy_chain: Vec::new(),
            param_chain: Vec::new(),
            in_route_group: false,
        }
    }
}

/// Two-pass recursive scan: the directory's companions first, then the rest.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn scan_directory(
    root: &Path,
    dir: &Path,
    descent: &Descent,
    tree: &mut RouteTree,
) -> Result<(), BuildError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| BuildError::io(dir, e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    let has_file = |name: &str| {
        entries
            .iter()
            .any(|entry| entry.file_name() == name && entry.path().is_file())
    };
    let has_index = has_file("index.rs");
    let has_layout = has_file("layout.rs");

    // Pass 1: this directory's companions apply to every child in it.
    let mut layout_chain = descent.layout_chain.clone();
    let mut policy_chain = descent.policy_chain.clone();
    let mut segment_claimed_by_layout: Option<PathBuf> = None;
    let mut index_loader: Option<PathBuf> = None;
    let mut frame: Option<PathBuf> = None;
    for entry in &entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name();
        let rel = relative(root, &path);
        match name.to_string_lossy().as_ref() {
            "layout.rs" => {
                let parsed = parse_file(&path, &rel)?;
                let layout_declares_slugs = declares_slugs(&parsed);
                if layout_declares_slugs && descent.in_route_group {
                    return Err(BuildError::SlugsInRouteGroup { file: rel });
                }
                if layout_declares_slugs {
                    segment_claimed_by_layout = Some(rel.clone());
                }
                tree.layouts.push(Layout {
                    file_path: rel.clone(),
                    url_prefix: descent.url_prefix.clone(),
                    declares_slugs: layout_declares_slugs,
                    slug_tokens: slug_tokens(&parsed),
                    frame: None,
                });
                layout_chain.push(rel);
            }
            "loader.rs" => {
                let parsed = parse_file(&path, &rel)?;
                let loads = exports_fn(&parsed, "load");
                let frames = exports_fn(&parsed, "frame");
                match (loads, frames) {
                    (false, false) => return Err(BuildError::LoaderServesNothing { file: rel }),
                    (true, _) if !has_index => {
                        return Err(BuildError::LoaderWithoutIndex { file: rel });
                    }
                    (_, true) if !has_layout => {
                        return Err(BuildError::FrameWithoutLayout { file: rel });
                    }
                    _ => {}
                }
                debug!(file = %rel.display(), loads, frames, "loader");
                if loads {
                    index_loader = Some(rel.clone());
                }
                if frames {
                    if dir == root {
                        return Err(BuildError::FrameOnRootLayout { file: rel });
                    }
                    frame = Some(rel.clone());
                }
                tree.loaders.push(rel);
            }
            "policy.rs" => {
                let parsed = parse_file(&path, &rel)?;
                if !exports_fn(&parsed, "policy") {
                    return Err(BuildError::PolicyWithoutFn { file: rel });
                }
                debug!(file = %rel.display(), "policy");
                tree.policies.push(rel.clone());
                policy_chain.push(rel);
            }
            _ => {}
        }
    }
    if let Some(frame) = frame
        && let Some(layout) = tree
            .layouts
            .iter_mut()
            .rev()
            .find(|layout| layout_chain.last() == Some(&layout.file_path))
    {
        layout.frame = Some(frame);
    }

    // Pass 2: subdirectories (recurse) and page/special files.
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            let (segment, captured) = dir_to_segment(&name);
            let url_prefix = if segment.is_empty() {
                descent.url_prefix.clone()
            } else {
                format_url(&descent.url_prefix, &segment)
            };
            let mut param_chain = descent.param_chain.clone();
            param_chain.extend(captured);
            let nested = Descent {
                url_prefix,
                layout_chain: layout_chain.clone(),
                policy_chain: policy_chain.clone(),
                param_chain,
                in_route_group: segment.is_empty(),
            };
            scan_directory(root, &path, &nested, tree)?;
            continue;
        }

        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }

        let rel = relative(root, &path);
        check_reserved(&name, &rel)?;

        if let Some(kind) = SpecialKind::classify(&name) {
            if !kind.is_companion() {
                handle_special(kind, &path, &rel, tree)?;
            }
            continue;
        }

        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        if stem == "mod" {
            continue;
        }

        let parsed = parse_file(&path, &rel)?;
        if page_fn_is_island(&parsed) {
            return Err(BuildError::IslandOnPage { file: rel });
        }
        let island = is_island(&parsed);
        if island {
            debug!(file = %rel.display(), "island opt-in");
        }
        let loads_data = defines_page_data(&parsed);
        if loads_data {
            debug!(file = %rel.display(), "page data opt-in");
        }
        let loader = if stem == "index" {
            index_loader.clone()
        } else {
            None
        };
        if loads_data && loader.is_some() {
            return Err(BuildError::LoaderBesideData { file: rel });
        }
        let page_declares_slugs = declares_slugs(&parsed);
        if page_declares_slugs {
            // A group adds no segment; its index page's segment belongs to the parent.
            if descent.in_route_group && stem == "index" {
                return Err(BuildError::SlugsInRouteGroup { file: rel });
            }
            // Two slug spellings for one segment would make one a hidden second address.
            if stem == "index"
                && let Some(layout) = segment_claimed_by_layout.take()
            {
                return Err(BuildError::SlugsDeclaredTwice { file: layout });
            }
            debug!(file = %rel.display(), "localized slugs opt-in");
        }

        let (url_path, kind, own_params) = file_to_route(&stem, &descent.url_prefix);
        // Path order, which is the order a link builder's arguments must be in.
        let mut params = descent.param_chain.clone();
        params.extend(own_params);
        let kind = match (kind, descent.param_chain.is_empty()) {
            // A static filename below a dynamic directory is still a dynamic route.
            (RouteKind::Static, false) => RouteKind::Dynamic,
            (kind, _) => kind,
        };
        tree.routes.push(Route {
            component_name: component_name(&url_path),
            url_path,
            module_path: rel,
            params,
            kind,
            layout_chain: layout_chain.clone(),
            island,
            declares_slugs: page_declares_slugs,
            slug_tokens: slug_tokens(&parsed),
            loads_data,
            policy_chain: policy_chain.clone(),
            loader,
        });
    }

    Ok(())
}

/// Refuse a page `x.rs` that sits beside a directory `x/`.
fn refuse_a_route_beside_its_directory(
    pages_dir: &Path,
    tree: &RouteTree,
) -> Result<(), BuildError> {
    match tree.routes_beside_their_directories(pages_dir).next() {
        Some((file, directory)) => Err(BuildError::RouteBesideItsDirectory {
            file: file.to_path_buf(),
            directory,
        }),
        None => Ok(()),
    }
}

/// Refuse a page that serves an address a `policy.rs` covers without that policy in its chain.
///
/// A capture never matches a literal (`/teams/new` is not `/teams/{team}`), and a policy in
/// a `(group)/` owns no address beyond the group's files.
fn refuse_a_route_outside_its_address_policy(tree: &RouteTree) -> Result<(), BuildError> {
    for policy in &tree.policies {
        let dir = policy.parent().unwrap_or(Path::new(""));
        let owns_an_address = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !(name.starts_with('(') && name.ends_with(')')));
        if !owns_an_address {
            continue;
        }
        let covered = address_segments_of_dir(dir);
        for route in &tree.routes {
            let served = address_segments(&route.url_path);
            let at_or_below = served.len() >= covered.len()
                && covered
                    .iter()
                    .zip(&served)
                    .all(|(policy, page)| policy == page);
            if at_or_below && !route.policy_chain.contains(policy) {
                return Err(BuildError::RouteOutsideItsAddressPolicy {
                    file: route.module_path.clone(),
                    address: route.url_path.clone(),
                    policy: policy.clone(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum AddressSegment<'a> {
    Literal(&'a str),
    Capture,
}

fn address_segments(url_path: &str) -> Vec<AddressSegment<'_>> {
    url_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| match segment.starts_with('{') {
            true => AddressSegment::Capture,
            false => AddressSegment::Literal(segment),
        })
        .collect()
}

fn address_segments_of_dir(dir: &Path) -> Vec<AddressSegment<'_>> {
    dir.components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|name| !(name.starts_with('(') && name.ends_with(')')))
        .map(|name| match name.starts_with('[') {
            true => AddressSegment::Capture,
            false => AddressSegment::Literal(name),
        })
        .collect()
}

/// Refuse a page that exports `Data` beneath a layout with a `frame`.
fn refuse_data_under_a_frame(tree: &RouteTree) -> Result<(), BuildError> {
    for route in tree.routes.iter().filter(|route| route.loads_data) {
        let framed = tree
            .layouts
            .iter()
            .filter(|layout| route.layout_chain.contains(&layout.file_path))
            .find_map(|layout| layout.frame.clone());
        if let Some(frame) = framed {
            return Err(BuildError::DataUnderFramedLayout {
                file: route.module_path.clone(),
                frame,
            });
        }
    }
    Ok(())
}

/// Record a root special file (`document.rs`/`not_found.rs`/`error.rs`/`gate.rs`).
fn handle_special(
    kind: SpecialKind,
    path: &Path,
    rel: &Path,
    tree: &mut RouteTree,
) -> Result<(), BuildError> {
    let parsed = parse_file(path, rel)?;
    let file = rel.to_path_buf();
    if kind == SpecialKind::NotFound && defines_page_data(&parsed) {
        return Err(BuildError::PageDataOnNotFound { file });
    }
    if kind == SpecialKind::Error && defines_page_data(&parsed) {
        return Err(BuildError::PageDataOnErrorPage { file });
    }
    if kind == SpecialKind::Gate {
        if rel
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
        {
            return Err(BuildError::GateNotAtRoot { file });
        }
        if defines_page_data(&parsed) {
            return Err(BuildError::PageDataOnGate { file });
        }
    }
    let slot = match kind {
        SpecialKind::Document => &mut tree.document,
        SpecialKind::NotFound => &mut tree.not_found,
        SpecialKind::Error => &mut tree.error_page,
        SpecialKind::Gate => &mut tree.gate,
        SpecialKind::Layout | SpecialKind::Loader | SpecialKind::Policy => return Ok(()),
    };
    if slot.is_some() {
        return Err(BuildError::DuplicateSpecial {
            kind: kind.label().to_string(),
            file,
        });
    }
    *slot = Some(file);
    Ok(())
}

fn parse_file(path: &Path, rel: &Path) -> Result<syn::File, BuildError> {
    let source = fs::read_to_string(path).map_err(|e| BuildError::io(path, e))?;
    syn::parse_file(&source).map_err(|e| BuildError::Parse {
        file: rel.to_path_buf(),
        message: e.to_string(),
    })
}

/// Convert a filename stem + URL prefix into `(url_path, kind, params)`.
///
/// Captures use the router's `{name}`/`{*name}` vocabulary, the only form
/// [`RoutePattern`](fundamental_server::RoutePattern) parses.
fn file_to_route(stem: &str, url_prefix: &str) -> (String, RouteKind, Vec<String>) {
    if stem == "index" {
        return (url_prefix.to_string(), RouteKind::Static, Vec::new());
    }
    if let Some(name) = stem.strip_prefix("[...").and_then(|s| s.strip_suffix(']')) {
        return (
            format_url(url_prefix, &format!("{{*{name}}}")),
            RouteKind::CatchAll,
            vec![name.to_string()],
        );
    }
    if let Some(name) = stem.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return (
            format_url(url_prefix, &format!("{{{name}}}")),
            RouteKind::Dynamic,
            vec![name.to_string()],
        );
    }
    (format_url(url_prefix, stem), RouteKind::Static, Vec::new())
}

/// Convert a directory name into `(url_segment, captured_param)`; a `(group)/` yields neither.
fn dir_to_segment(name: &str) -> (String, Option<String>) {
    if name.starts_with('(') && name.ends_with(')') {
        return (String::new(), None);
    }
    if let Some(param) = name.strip_prefix("[...").and_then(|s| s.strip_suffix(']')) {
        (format!("{{*{param}}}"), Some(param.to_string()))
    } else if let Some(param) = name.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        (format!("{{{param}}}"), Some(param.to_string()))
    } else {
        (name.to_string(), None)
    }
}

fn format_url(prefix: &str, segment: &str) -> String {
    if prefix == "/" {
        format!("/{segment}")
    } else {
        format!("{prefix}/{segment}")
    }
}

fn relative(root: &Path, full: &Path) -> PathBuf {
    full.strip_prefix(root).unwrap_or(full).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;
    use tempfile::TempDir;

    const PAGE: &str = "pub fn page() {}";

    fn write(dir: &Path, rel: &str, src: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, src).unwrap();
    }

    fn page(dir: &Path, rel: &str) {
        write(dir, rel, PAGE);
    }

    fn scan(dir: &Path) -> RouteTree {
        RouteTree::scan(dir).unwrap()
    }

    #[test]
    fn index_maps_to_root() {
        let d = TempDir::new().unwrap();
        page(d.path(), "index.rs");
        let t = scan(d.path());
        assert_eq!(t.routes.len(), 1);
        assert_eq!(t.routes[0].url_path, "/");
        assert_eq!(t.routes[0].component_name, "IndexPage");
        assert_eq!(t.routes[0].kind, RouteKind::Static);
    }

    #[test]
    fn static_page() {
        let d = TempDir::new().unwrap();
        page(d.path(), "about.rs");
        let t = scan(d.path());
        assert_eq!(t.routes[0].url_path, "/about");
        assert_eq!(t.routes[0].component_name, "AboutPage");
    }

    #[test]
    fn dynamic_segment() {
        let d = TempDir::new().unwrap();
        page(d.path(), "[id].rs");
        let t = scan(d.path());
        assert_eq!(t.routes[0].url_path, "/{id}");
        assert_eq!(t.routes[0].params, vec!["id"]);
        assert_eq!(t.routes[0].kind, RouteKind::Dynamic);
    }

    #[test]
    fn catch_all_segment() {
        let d = TempDir::new().unwrap();
        page(d.path(), "[...slug].rs");
        let t = scan(d.path());
        assert_eq!(t.routes[0].url_path, "/{*slug}");
        assert_eq!(t.routes[0].params, vec!["slug"]);
        assert_eq!(t.routes[0].kind, RouteKind::CatchAll);
        assert_eq!(t.routes[0].component_name, "SlugPage");
    }

    #[test]
    fn nested_directory() {
        let d = TempDir::new().unwrap();
        page(d.path(), "users/index.rs");
        page(d.path(), "users/[id].rs");
        page(d.path(), "users/new.rs");
        let t = scan(d.path());
        let paths: Vec<&str> = t.routes.iter().map(|r| r.url_path.as_str()).collect();
        assert!(paths.contains(&"/users"));
        assert!(paths.contains(&"/users/new"));
        assert!(paths.contains(&"/users/{id}"));
        let idx = t.routes.iter().find(|r| r.url_path == "/users").unwrap();
        assert_eq!(idx.component_name, "UsersPage");
    }

    #[test]
    fn route_group_contributes_no_segment_but_passes_layout() {
        let d = TempDir::new().unwrap();
        write(d.path(), "(marketing)/layout.rs", PAGE);
        page(d.path(), "(marketing)/pricing.rs");
        let t = scan(d.path());
        let r = t.routes.iter().find(|r| r.url_path == "/pricing").unwrap();
        assert_eq!(r.layout_chain.len(), 1, "group layout should wrap the page");
        assert!(t.routes.iter().all(|r| !r.url_path.contains("marketing")));
    }

    #[test]
    fn layout_chain_accumulates_with_depth() {
        let d = TempDir::new().unwrap();
        write(d.path(), "layout.rs", PAGE);
        write(d.path(), "users/layout.rs", PAGE);
        page(d.path(), "users/[id].rs");
        let t = scan(d.path());
        let r = t
            .routes
            .iter()
            .find(|r| r.url_path == "/users/{id}")
            .unwrap();
        assert_eq!(r.layout_chain.len(), 2);
        assert!(r.layout_chain[0].ends_with("layout.rs"));
        assert!(r.layout_chain[1].to_string_lossy().contains("users"));
        assert_eq!(t.layouts.len(), 2);
    }

    #[test]
    fn sort_order_static_dynamic_catch_all() {
        let d = TempDir::new().unwrap();
        page(d.path(), "[...rest].rs");
        page(d.path(), "[id].rs");
        page(d.path(), "about.rs");
        page(d.path(), "index.rs");
        let t = scan(d.path());
        let order: Vec<&str> = t.routes.iter().map(|r| r.url_path.as_str()).collect();
        assert_eq!(order, vec!["/", "/about", "/{id}", "/{*rest}"]);
    }

    #[test]
    fn mod_rs_is_skipped() {
        let d = TempDir::new().unwrap();
        page(d.path(), "mod.rs");
        page(d.path(), "index.rs");
        let t = scan(d.path());
        assert_eq!(t.routes.len(), 1);
    }

    #[test]
    fn document_detected() {
        let d = TempDir::new().unwrap();
        page(d.path(), "document.rs");
        page(d.path(), "index.rs");
        let t = scan(d.path());
        assert!(t.document.is_some());
        assert_eq!(t.routes.len(), 1);
    }

    #[test]
    fn not_found_detected() {
        let d = TempDir::new().unwrap();
        page(d.path(), "not_found.rs");
        let t = scan(d.path());
        assert!(t.not_found.is_some());
    }

    #[test]
    fn error_page_detected() {
        let d = TempDir::new().unwrap();
        page(d.path(), "error.rs");
        let t = scan(d.path());
        assert!(t.error_page.is_some());
        assert!(t.routes.is_empty(), "the error page is never a route");
    }

    #[test]
    fn duplicate_document_errors() {
        let d = TempDir::new().unwrap();
        page(d.path(), "document.rs");
        page(d.path(), "sub/document.rs");
        let err = RouteTree::scan(d.path()).unwrap_err();
        match err {
            BuildError::DuplicateSpecial { kind, .. } => assert_eq!(kind, "document"),
            other => panic!("expected DuplicateSpecial, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_not_found_errors() {
        let d = TempDir::new().unwrap();
        page(d.path(), "not_found.rs");
        page(d.path(), "deep/not_found.rs");
        let err = RouteTree::scan(d.path()).unwrap_err();
        assert!(matches!(err, BuildError::DuplicateSpecial { .. }));
    }

    #[test]
    fn reserved_names_each_error() {
        for name in ["loading.rs", "_shell.rs", "not-found.rs"] {
            let d = TempDir::new().unwrap();
            page(d.path(), name);
            let err = RouteTree::scan(d.path()).unwrap_err();
            assert!(matches!(err, BuildError::ReservedFile { .. }), "{name}");
        }
    }

    #[test]
    fn pages_are_server_rendered_by_default() {
        let d = TempDir::new().unwrap();
        page(d.path(), "index.rs");
        let t = scan(d.path());
        assert!(!t.routes[0].island, "no #[island] means server-rendered");
    }

    #[test]
    fn island_opt_in_read_from_page() {
        let d = TempDir::new().unwrap();
        write(d.path(), "live.rs", "#[island] pub fn live() {}");
        let t = scan(d.path());
        assert!(t.routes[0].island);
    }

    #[test]
    fn island_on_the_page_fn_is_a_clear_error() {
        let d = TempDir::new().unwrap();
        write(d.path(), "index.rs", "#[island] pub fn page() {}");
        let err = RouteTree::scan(d.path()).unwrap_err();
        match err {
            BuildError::IslandOnPage { file } => assert!(file.ends_with("index.rs")),
            other => panic!("expected IslandOnPage, got {other:?}"),
        }
    }

    #[test]
    fn an_island_component_beside_a_plain_page_is_fine() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "index.rs",
            "#[island] fn Widget() {}\npub fn page() {}",
        );
        let t = scan(d.path());
        assert_eq!(t.routes.len(), 1);
        assert!(t.routes[0].island, "the page ships an island bundle");
    }

    #[test]
    fn syntax_error_is_parse_error() {
        let d = TempDir::new().unwrap();
        write(d.path(), "broken.rs", "pub fn page( {");
        let err = RouteTree::scan(d.path()).unwrap_err();
        match err {
            BuildError::Parse { file, .. } => assert!(file.ends_with("broken.rs")),
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn missing_pages_dir_is_empty_tree() {
        let t = RouteTree::scan(Path::new("/nonexistent/pages")).unwrap();
        assert!(t.routes.is_empty());
        assert!(t.layouts.is_empty());
        assert!(t.document.is_none());
    }

    #[test]
    fn deeply_nested_dynamic_params() {
        let d = TempDir::new().unwrap();
        page(d.path(), "[org]/projects/[project_id]/tasks/[task_id].rs");
        let t = scan(d.path());
        assert_eq!(
            t.routes[0].url_path,
            "/{org}/projects/{project_id}/tasks/{task_id}"
        );
        assert_eq!(
            t.routes[0].params,
            vec!["org", "project_id", "task_id"],
            "directory captures belong to the route as much as the filename's does"
        );
    }

    #[test]
    fn a_static_filename_below_a_dynamic_directory_is_a_dynamic_route() {
        let d = TempDir::new().unwrap();
        page(d.path(), "[org]/settings.rs");
        page(d.path(), "admin/settings.rs");
        let t = scan(d.path());

        let scoped = t
            .routes
            .iter()
            .find(|r| r.url_path == "/{org}/settings")
            .expect("the parameterized path is discovered");
        assert_eq!(scoped.params, vec!["org"]);
        assert_eq!(
            scoped.kind,
            RouteKind::Dynamic,
            "it carries a capture, so it must not be classified as literal"
        );

        let order: Vec<&str> = t.routes.iter().map(|r| r.url_path.as_str()).collect();
        assert_eq!(order, vec!["/admin/settings", "/{org}/settings"]);
    }

    #[test]
    fn a_page_declaring_slugs_is_recorded() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "why.rs",
            "slugs! { nl = \"waarom\" }\npub fn page() {}",
        );
        page(d.path(), "trust.rs");
        let t = scan(d.path());

        let why = t
            .routes
            .iter()
            .find(|r| r.url_path == "/why")
            .expect("discovered");
        assert!(why.declares_slugs);
        assert_eq!(why.slug_tokens.as_deref(), Some("nl = \"waarom\""));
        let trust = t
            .routes
            .iter()
            .find(|r| r.url_path == "/trust")
            .expect("discovered");
        assert!(!trust.declares_slugs);
        assert_eq!(trust.slug_tokens, None);
    }

    #[test]
    fn a_layout_declaring_slugs_is_recorded() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "guides/layout.rs",
            "slugs! { nl = \"gidsen\" }\npub fn layout<V>(v: V) -> V { v }",
        );
        page(d.path(), "guides/api.rs");
        let t = scan(d.path());
        assert!(t.layouts.iter().any(|layout| layout.declares_slugs
            && layout.slug_tokens.as_deref() == Some("nl = \"gidsen\"")));
    }

    #[test]
    fn slugs_on_a_route_groups_own_files_are_refused() {
        for (file, source) in [
            (
                "(marketing)/index.rs",
                "slugs! { nl = \"start\" }\npub fn page() {}",
            ),
            (
                "(marketing)/layout.rs",
                "slugs! { nl = \"start\" }\npub fn layout<V>(v: V) -> V { v }",
            ),
        ] {
            let d = TempDir::new().unwrap();
            write(d.path(), file, source);
            let error = RouteTree::scan(d.path()).expect_err("{file} should be refused");
            assert!(
                matches!(error, BuildError::SlugsInRouteGroup { .. }),
                "{file} gave {error:?}"
            );
        }
    }

    #[test]
    fn a_directory_segment_claimed_by_both_its_index_and_its_layout_is_refused() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "guides/layout.rs",
            "slugs! { nl = \"gidsen\" }\npub fn layout<V>(v: V) -> V { v }",
        );
        write(
            d.path(),
            "guides/index.rs",
            "slugs! { nl = \"handleidingen\" }\npub fn page() {}",
        );
        let error = RouteTree::scan(d.path()).expect_err("one segment, two spellings");
        assert!(
            matches!(error, BuildError::SlugsDeclaredTwice { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_layout_slug_coexists_with_a_sibling_pages_own_slug() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "guides/layout.rs",
            "slugs! { nl = \"gidsen\" }\npub fn layout<V>(v: V) -> V { v }",
        );
        write(
            d.path(),
            "guides/api.rs",
            "slugs! { nl = \"koppeling\" }\npub fn page() {}",
        );
        let t = scan(d.path());
        assert!(t.layouts.iter().any(|layout| layout.declares_slugs));
        assert!(
            t.routes
                .iter()
                .any(|r| r.url_path == "/guides/api" && r.declares_slugs)
        );
    }

    #[test]
    fn a_page_inside_a_route_group_may_still_localize_its_own_segment() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "(marketing)/pricing.rs",
            "slugs! { nl = \"prijzen\" }\npub fn page() {}",
        );
        let t = scan(d.path());
        let pricing = t
            .routes
            .iter()
            .find(|r| r.url_path == "/pricing")
            .expect("discovered");
        assert!(pricing.declares_slugs);
    }

    #[test]
    fn a_scanned_template_is_the_one_the_router_parses() {
        let d = TempDir::new().unwrap();
        page(d.path(), "users/[id].rs");
        page(d.path(), "blog/[...slug].rs");
        let t = scan(d.path());

        for route in &t.routes {
            for param in &route.params {
                assert!(
                    route.url_path.contains(&format!("{{{param}}}"))
                        || route.url_path.contains(&format!("{{*{param}}}")),
                    "{} does not capture {param} in the router's vocabulary",
                    route.url_path
                );
            }
            assert!(
                !route.url_path.contains(':') && !route.url_path.contains("/*"),
                "{} still carries a marker the router does not parse",
                route.url_path
            );
        }
    }

    const LOADER: &str = "pub async fn load() {}";
    const POLICY: &str = "pub fn policy() {}";

    fn refusal(dir: &Path) -> BuildError {
        RouteTree::scan(dir).expect_err("the tree is refused")
    }

    fn route<'a>(tree: &'a RouteTree, url: &str) -> &'a Route {
        tree.routes
            .iter()
            .find(|route| route.url_path == url)
            .unwrap_or_else(|| panic!("no route {url}"))
    }

    #[test]
    fn a_loader_is_a_companion_not_a_route() {
        let d = TempDir::new().unwrap();
        page(d.path(), "teams/[team]/index.rs");
        page(d.path(), "teams/[team]/members.rs");
        write(d.path(), "teams/[team]/loader.rs", LOADER);
        let t = scan(d.path());
        assert_eq!(t.routes.len(), 2, "{:?}", t.routes);
        assert_eq!(
            route(&t, "/teams/{team}").loader,
            Some(PathBuf::from("teams/[team]/loader.rs"))
        );
        assert_eq!(route(&t, "/teams/{team}/members").loader, None);
        assert_eq!(t.loaders, vec![PathBuf::from("teams/[team]/loader.rs")]);
        assert!(t.serves_page_state());
    }

    #[test]
    fn a_frame_is_handed_to_the_layout_beside_it() {
        let d = TempDir::new().unwrap();
        write(d.path(), "layout.rs", "pub fn layout() {}");
        page(d.path(), "teams/index.rs");
        write(d.path(), "teams/layout.rs", "pub fn layout() {}");
        write(d.path(), "teams/loader.rs", "pub async fn frame() {}");
        let t = scan(d.path());
        let frames: Vec<_> = t.layouts.iter().map(|l| l.frame.clone()).collect();
        assert_eq!(frames, vec![None, Some(PathBuf::from("teams/loader.rs"))]);
        assert_eq!(
            route(&t, "/teams").loader,
            None,
            "a frame is not a page's loader"
        );
    }

    #[test]
    fn a_loader_without_index_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "teams/list.rs");
        write(d.path(), "teams/loader.rs", LOADER);
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::LoaderWithoutIndex { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_015");
        assert!(err.to_string().contains("teams/loader.rs"), "{err}");
    }

    #[test]
    fn a_frame_without_layout_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "teams/index.rs");
        write(
            d.path(),
            "teams/loader.rs",
            "pub async fn load() {}\npub async fn frame() {}",
        );
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::FrameWithoutLayout { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_016");
    }

    #[test]
    fn a_loader_serving_nothing_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "index.rs");
        write(d.path(), "loader.rs", "pub async fn fetch() {}");
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::LoaderServesNothing { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_017");
    }

    #[test]
    fn a_loader_beside_data_is_refused() {
        let d = TempDir::new().unwrap();
        write(
            d.path(),
            "index.rs",
            "pub type Data = u8;\npub fn page() {}",
        );
        write(d.path(), "loader.rs", LOADER);
        let err = refusal(d.path());
        assert!(matches!(err, BuildError::LoaderBesideData { .. }), "{err}");
        assert_eq!(err.error_code(), "BUILD_018");
        assert!(err.to_string().contains("index.rs"), "{err}");
    }

    #[test]
    fn a_policy_covers_every_route_below_it() {
        let d = TempDir::new().unwrap();
        write(d.path(), "policy.rs", POLICY);
        page(d.path(), "index.rs");
        write(d.path(), "teams/[team]/policy.rs", POLICY);
        page(d.path(), "teams/[team]/index.rs");
        page(d.path(), "teams/[team]/settings/index.rs");
        page(d.path(), "teams/[team]/[project].rs");
        let t = scan(d.path());
        let root = PathBuf::from("policy.rs");
        let team = PathBuf::from("teams/[team]/policy.rs");
        assert_eq!(route(&t, "/").policy_chain, vec![root.clone()]);
        for url in [
            "/teams/{team}",
            "/teams/{team}/settings",
            "/teams/{team}/{project}",
        ] {
            assert_eq!(
                route(&t, url).policy_chain,
                vec![root.clone(), team.clone()],
                "{url}"
            );
        }
        assert_eq!(t.policies, vec![root, team]);
    }

    #[test]
    fn a_group_policy_covers_its_routes() {
        let d = TempDir::new().unwrap();
        page(d.path(), "about.rs");
        write(d.path(), "(desk)/policy.rs", POLICY);
        page(d.path(), "(desk)/inbox.rs");
        let t = scan(d.path());
        assert_eq!(
            route(&t, "/inbox").policy_chain,
            vec![PathBuf::from("(desk)/policy.rs")]
        );
        assert!(route(&t, "/about").policy_chain.is_empty());
    }

    #[test]
    fn a_policy_without_its_fn_is_refused() {
        let d = TempDir::new().unwrap();
        write(d.path(), "policy.rs", "pub fn page() {}");
        let err = refusal(d.path());
        assert!(matches!(err, BuildError::PolicyWithoutFn { .. }), "{err}");
        assert_eq!(err.error_code(), "BUILD_022");
        assert!(err.to_string().contains("policy/index.rs"), "{err}");
    }

    #[test]
    fn a_route_beside_its_directory_is_refused_in_a_policy_tree() {
        let d = TempDir::new().unwrap();
        page(d.path(), "staff.rs");
        write(d.path(), "staff/policy.rs", POLICY);
        page(d.path(), "staff/inbox.rs");
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::RouteBesideItsDirectory { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_023");
        assert!(err.to_string().contains("staff.rs"), "{err}");

        let dynamic = TempDir::new().unwrap();
        page(dynamic.path(), "teams/[team].rs");
        page(dynamic.path(), "teams/[team]/index.rs");
        write(dynamic.path(), "teams/[team]/loader.rs", LOADER);
        assert!(matches!(
            refusal(dynamic.path()),
            BuildError::RouteBesideItsDirectory { .. }
        ));
    }

    #[test]
    fn a_route_beside_its_directory_still_builds_in_a_tree_without_policies() {
        let d = TempDir::new().unwrap();
        page(d.path(), "staff.rs");
        page(d.path(), "staff/inbox.rs");
        let t = scan(d.path());
        assert_eq!(t.routes.len(), 2);
        assert!(!t.serves_page_state());
    }

    #[test]
    fn the_gate_is_a_root_special_page() {
        let d = TempDir::new().unwrap();
        page(d.path(), "index.rs");
        page(d.path(), "error.rs");
        page(d.path(), "gate.rs");
        let t = scan(d.path());
        assert_eq!(t.gate, Some(PathBuf::from("gate.rs")));
        assert_eq!(t.routes.len(), 1, "the gate is not routed");
        assert!(t.serves_page_state());
    }

    #[test]
    fn a_nested_gate_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "error.rs");
        page(d.path(), "teams/gate.rs");
        let err = refusal(d.path());
        assert!(matches!(err, BuildError::GateNotAtRoot { .. }), "{err}");
        assert_eq!(err.error_code(), "BUILD_019");
    }

    #[test]
    fn a_gate_with_data_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "error.rs");
        write(d.path(), "gate.rs", "pub type Data = u8;\npub fn page() {}");
        let err = refusal(d.path());
        assert!(matches!(err, BuildError::PageDataOnGate { .. }), "{err}");
        assert_eq!(err.error_code(), "BUILD_020");
    }

    #[test]
    fn a_gate_without_an_error_page_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "gate.rs");
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::GateWithoutErrorPage { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_021");
    }

    #[test]
    fn a_frame_on_the_root_layout_is_refused() {
        let d = TempDir::new().unwrap();
        page(d.path(), "index.rs");
        write(d.path(), "layout.rs", "pub fn layout() {}");
        write(d.path(), "loader.rs", "pub async fn frame() {}");
        let err = refusal(d.path());
        assert!(matches!(err, BuildError::FrameOnRootLayout { .. }), "{err}");
        assert_eq!(err.error_code(), "BUILD_024");
    }

    #[test]
    fn data_under_a_framed_layout_is_refused() {
        let d = TempDir::new().unwrap();
        write(d.path(), "(app)/layout.rs", "pub fn layout() {}");
        write(d.path(), "(app)/loader.rs", "pub async fn frame() {}");
        page(d.path(), "(app)/index.rs");
        write(
            d.path(),
            "(app)/devices.rs",
            "pub type Data = u8;\npub fn page() {}",
        );
        let err = refusal(d.path());
        assert!(
            matches!(err, BuildError::DataUnderFramedLayout { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_025");
    }

    #[test]
    fn a_route_outside_its_address_policy_is_refused() {
        // The page is at the root, the policy below a group: both are the address `/staff`.
        let into_a_group = TempDir::new().unwrap();
        page(into_a_group.path(), "staff.rs");
        write(into_a_group.path(), "(desk)/staff/policy.rs", POLICY);
        page(into_a_group.path(), "(desk)/staff/inbox.rs");
        let err = refusal(into_a_group.path());
        assert!(
            matches!(err, BuildError::RouteOutsideItsAddressPolicy { .. }),
            "{err}"
        );
        assert_eq!(err.error_code(), "BUILD_026");
        assert!(err.to_string().contains("(desk)/staff/policy.rs"), "{err}");

        // The page in a group, the policy outside it.
        let out_of_a_group = TempDir::new().unwrap();
        page(out_of_a_group.path(), "(public)/staff.rs");
        write(out_of_a_group.path(), "staff/policy.rs", POLICY);
        page(out_of_a_group.path(), "staff/inbox.rs");
        assert!(matches!(
            refusal(out_of_a_group.path()),
            BuildError::RouteOutsideItsAddressPolicy { .. }
        ));
    }

    #[test]
    fn a_static_route_beside_a_captured_policy_is_its_own_address() {
        let d = TempDir::new().unwrap();
        page(d.path(), "teams/new.rs");
        write(d.path(), "teams/[team]/policy.rs", POLICY);
        page(d.path(), "teams/[team]/index.rs");
        let t = scan(d.path());
        assert!(route(&t, "/teams/new").policy_chain.is_empty());
    }

    #[test]
    fn a_policy_of_a_group_covers_only_the_group() {
        let d = TempDir::new().unwrap();
        page(d.path(), "about.rs");
        write(d.path(), "teams/(admin)/policy.rs", POLICY);
        page(d.path(), "teams/(admin)/settings.rs");
        page(d.path(), "teams/list.rs");
        let t = scan(d.path());
        assert!(route(&t, "/teams/list").policy_chain.is_empty());
        assert_eq!(route(&t, "/teams/settings").policy_chain.len(), 1);
    }
}
