//! Detect which functions a page-tree companion file exports; only the name is read.

use syn::{File, Item, UseTree, Visibility};

/// Whether `file` exports a public function `name`, directly or by a `pub use` re-export.
pub(crate) fn exports_fn(file: &File, name: &str) -> bool {
    file.items.iter().any(|item| match item {
        Item::Fn(function) => is_public(&function.vis) && function.sig.ident == name,
        Item::Use(item) => is_public(&item.vis) && use_tree_binds(&item.tree, name),
        _ => false,
    })
}

/// Whether a visibility is plain `pub`; a narrower one may resolve in one including module
/// and not the other.
pub(crate) fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Whether a `use` tree binds `name`, directly or through `as name`; a glob never counts.
pub(crate) fn use_tree_binds(tree: &UseTree, name: &str) -> bool {
    match tree {
        UseTree::Path(path) => use_tree_binds(&path.tree, name),
        UseTree::Name(bound) => bound.ident == name,
        UseTree::Rename(rename) => rename.rename == name,
        UseTree::Group(group) => group.items.iter().any(|tree| use_tree_binds(tree, name)),
        UseTree::Glob(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::exports_fn;
    use syn::File;

    fn parse(src: &str) -> File {
        syn::parse_file(src).expect("valid source")
    }

    #[test]
    fn a_public_async_fn_is_an_export() {
        let file = parse("pub async fn load() {}\npub async fn frame() {}");
        assert!(exports_fn(&file, "load"));
        assert!(exports_fn(&file, "frame"));
        assert!(!exports_fn(&file, "policy"));
    }

    #[test]
    fn a_re_export_counts_including_under_a_rename() {
        assert!(exports_fn(&parse("pub use crate::team::load;"), "load"));
        assert!(exports_fn(
            &parse("pub use crate::team::overview as load;"),
            "load"
        ));
        assert!(exports_fn(
            &parse("pub use crate::team::{a, policy};"),
            "policy"
        ));
    }

    #[test]
    fn a_private_fn_or_a_glob_is_not_an_export() {
        assert!(!exports_fn(&parse("async fn load() {}"), "load"));
        assert!(!exports_fn(&parse("pub(crate) fn policy() {}"), "policy"));
        assert!(!exports_fn(&parse("pub use crate::team::*;"), "load"));
    }

    #[test]
    fn another_item_with_the_name_is_not_a_function() {
        assert!(!exports_fn(&parse("pub struct load;"), "load"));
        assert!(!exports_fn(&parse("pub const policy: u8 = 0;"), "policy"));
    }
}
