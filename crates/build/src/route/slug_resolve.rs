//! Detect a page's localized-slug declaration in an already-parsed page file.
//!
//! Only whether one is there: the slug values reach the table from the `slugs!` expansion.

use syn::{File, Item, Visibility};

const PAGE_SLUGS: &str = "PAGE_SLUGS";

/// Whether the page file declares localized slugs, by `slugs!` or a hand-written `pub const PAGE_SLUGS`.
pub(crate) fn declares_slugs(parsed: &File) -> bool {
    parsed.items.iter().any(|item| match item {
        Item::Macro(item) => is_slugs_macro(item),
        Item::Const(item) => matches!(item.vis, Visibility::Public(_)) && item.ident == PAGE_SLUGS,
        _ => false,
    })
}

/// The tokens inside the page's `slugs!` invocation, verbatim, for a routes crate to expand;
/// `None` without one.
pub(crate) fn slug_tokens(parsed: &File) -> Option<String> {
    parsed.items.iter().find_map(|item| match item {
        Item::Macro(item) if is_slugs_macro(item) => Some(item.mac.tokens.to_string()),
        _ => None,
    })
}

/// Whether an item macro is a `slugs!` invocation, matched on its last path segment.
fn is_slugs_macro(item: &syn::ItemMacro) -> bool {
    item.mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "slugs")
}

#[cfg(test)]
mod tests {
    use super::{declares_slugs, slug_tokens};
    use syn::File;

    fn parse(src: &str) -> File {
        syn::parse_file(src).expect("valid source")
    }

    #[test]
    fn a_page_with_no_slugs_declares_nothing() {
        assert!(!declares_slugs(&parse("pub fn page() {}")));
    }

    #[test]
    fn a_slugs_invocation_is_a_declaration_however_it_is_pathed() {
        for source in [
            "slugs! { nl = \"waarom\" }\npub fn page() {}",
            "fundamental::slugs! { nl = \"waarom\" }\npub fn page() {}",
            "::fundamental::slugs! { nl = \"waarom\" }\npub fn page() {}",
        ] {
            assert!(declares_slugs(&parse(source)), "{source}");
        }
    }

    #[test]
    fn a_hand_written_constant_declares_it_just_as_well() {
        assert!(declares_slugs(&parse(
            "pub const PAGE_SLUGS: &[PageSlug] = &[];"
        )));
    }

    /// A private constant is not a declaration to the generator.
    #[test]
    fn a_private_constant_is_not_a_declaration() {
        assert!(!declares_slugs(&parse("const PAGE_SLUGS: &[u8] = &[];")));
        assert!(!declares_slugs(&parse(
            "pub(crate) const PAGE_SLUGS: &[u8] = &[];"
        )));
    }

    #[test]
    fn the_tokens_of_a_slugs_invocation_are_carried_verbatim() {
        let tokens = slug_tokens(&parse(
            "fundamental::slugs! { en = \"why\", retired en = \"reason\" }\npub fn page() {}",
        ))
        .expect("an invocation");
        assert_eq!(tokens, "en = \"why\" , retired en = \"reason\"");
        assert_eq!(slug_tokens(&parse("pub fn page() {}")), None);
        assert_eq!(
            slug_tokens(&parse("pub const PAGE_SLUGS: &[PageSlug] = &[];")),
            None,
            "a hand-written constant has no tokens to carry"
        );
    }

    #[test]
    fn another_macro_is_not_a_declaration() {
        assert!(!declares_slugs(&parse("mount_actions! { Double }")));
    }

    #[test]
    fn a_differently_named_constant_is_not_a_declaration() {
        assert!(!declares_slugs(&parse("pub const SLUGS: &[u8] = &[];")));
    }
}
