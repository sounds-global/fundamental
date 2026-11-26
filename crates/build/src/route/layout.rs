//! [`Layout`], a `layout.rs` wrapper discovered during scanning.

use std::path::PathBuf;

/// A `layout.rs` file and the URL prefix at which it applies.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layout {
    /// The layout file, relative to the pages dir.
    pub file_path: PathBuf,
    /// The URL prefix the layout wraps, e.g. `/` or `/users`.
    pub url_prefix: String,
    /// Whether the layout declares localized slugs with `slugs!`.
    pub declares_slugs: bool,
    /// The tokens inside the layout's `slugs!` invocation, verbatim.
    pub slug_tokens: Option<String>,
    /// The `loader.rs` beside this layout when it exports `frame`, relative to the pages dir.
    pub frame: Option<PathBuf>,
}
