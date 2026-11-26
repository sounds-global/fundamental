//! [`Route`], a single discovered page route in the IR.

use std::path::PathBuf;

use super::RouteKind;

/// A single page route discovered from the filesystem.
/// A single page route discovered from the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Route {
    /// The URL pattern, e.g. `/`, `/about`, `/users/{id}`, `/blog/{*slug}`.
    pub url_path: String,
    /// The page file, relative to the pages dir, e.g. `users/[id].rs`.
    pub module_path: PathBuf,
    /// A unique PascalCase component name derived from the path, e.g. `UsersIdPage`.
    pub component_name: String,
    /// Dynamic/catch-all parameter names in order, e.g. `["id"]` or `["slug"]`.
    pub params: Vec<String>,
    /// How this route matches incoming paths.
    pub kind: RouteKind,
    /// `layout.rs` files wrapping this route, outermost first, relative to the pages dir.
    pub layout_chain: Vec<PathBuf>,
    /// Whether the page opted into client-side rendering with `#[island]`.
    pub island: bool,
    /// Whether the page declares localized slugs with `slugs!`.
    pub declares_slugs: bool,
    /// The tokens inside the page's `slugs!` invocation, verbatim, for a routes crate to expand.
    pub slug_tokens: Option<String>,
    /// Whether the page exports a public `Data` type, loaded before it renders.
    pub loads_data: bool,
    /// The `policy.rs` files at or above this route, outermost first, relative to the pages dir.
    pub policy_chain: Vec<PathBuf>,
    /// The `loader.rs` whose `load` serves this page (only an `index.rs` has one).
    pub loader: Option<PathBuf>,
}
