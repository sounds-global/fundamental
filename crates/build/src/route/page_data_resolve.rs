//! Detect a page's data type in an already-parsed page file: an exported `Data`, which the
//! generated route loads through [`PageData`](fundamental_web::PageData).

use syn::{File, Item};

use super::loader_resolve::{is_public, use_tree_binds};

const DATA: &str = "Data";

/// Whether the page file exports a public `Data` type (an alias, `struct`, `enum` or `use`).
pub(crate) fn defines_page_data(parsed: &File) -> bool {
    parsed.items.iter().any(|item| match item {
        Item::Type(alias) => is_public(&alias.vis) && alias.ident == DATA,
        Item::Struct(item) => is_public(&item.vis) && item.ident == DATA,
        Item::Enum(item) => is_public(&item.vis) && item.ident == DATA,
        Item::Use(item) => is_public(&item.vis) && use_tree_binds(&item.tree, DATA),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::defines_page_data;
    use syn::File;

    fn parse(src: &str) -> File {
        syn::parse_file(src).expect("valid source")
    }

    #[test]
    fn a_page_with_no_data_is_generated_as_it_always_was() {
        assert!(!defines_page_data(&parse("pub fn page() {}")));
    }

    #[test]
    fn an_alias_is_the_ordinary_form() {
        let file = parse("pub type Data = crate::SecurityView;\npub fn page() {}");
        assert!(defines_page_data(&file));
    }

    #[test]
    fn a_struct_or_an_enum_declares_it_just_as_well() {
        assert!(defines_page_data(&parse("pub struct Data;")));
        assert!(defines_page_data(&parse("pub enum Data { Empty }")));
    }

    #[test]
    fn a_re_export_declares_it_including_under_a_rename() {
        assert!(defines_page_data(&parse("pub use crate::view::Data;")));
        assert!(defines_page_data(&parse(
            "pub use crate::view::SecurityView as Data;"
        )));
        assert!(defines_page_data(&parse(
            "pub use crate::view::{Other, Data};"
        )));
    }

    /// A private type is a page's own business, not a declaration to the generator.
    #[test]
    fn a_private_data_type_is_not_a_declaration() {
        assert!(!defines_page_data(&parse("type Data = u8;")));
        assert!(!defines_page_data(&parse("struct Data;")));
        assert!(!defines_page_data(&parse("pub(crate) type Data = u8;")));
    }

    /// A glob says nothing about what it brings in, so it cannot be read as a promise.
    #[test]
    fn a_glob_import_is_not_a_declaration() {
        assert!(!defines_page_data(&parse("pub use crate::view::*;")));
    }
}
