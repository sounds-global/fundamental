//! Find the page-tree files that declare an `#[island]`.

use std::fs;
use std::path::{Path, PathBuf};

use super::island_resolve::is_island;
use super::route_tree::RouteTree;
use crate::build_error::BuildError;

/// Every file of `tree` (page, layout, 404 page and gate) that declares an `#[island]`,
/// relative to `pages_dir`.
///
/// # Errors
/// [`BuildError::Io`] when a file cannot be read, [`BuildError::Parse`] when it is not Rust.
pub(crate) fn islands_in_pages(
    tree: &RouteTree,
    pages_dir: &Path,
) -> Result<Vec<PathBuf>, BuildError> {
    let files = tree
        .routes
        .iter()
        .map(|route| &route.module_path)
        .chain(tree.layouts.iter().map(|layout| &layout.file_path))
        .chain(&tree.not_found)
        .chain(&tree.gate);
    let mut found = Vec::new();
    for rel in files {
        let path = pages_dir.join(rel);
        let source = fs::read_to_string(&path).map_err(|e| BuildError::io(&path, e))?;
        let parsed = syn::parse_file(&source).map_err(|e| BuildError::Parse {
            file: rel.clone(),
            message: e.to_string(),
        })?;
        if is_island(&parsed) {
            found.push(rel.clone());
        }
    }
    Ok(found)
}
