//! Detect islands in an already-parsed page file.

use syn::{File, Item};

/// Whether the page file contains any `#[island]` component.
pub(crate) fn is_island(parsed: &File) -> bool {
    parsed.items.iter().any(|item| {
        let Item::Fn(func) = item else { return false };
        func.attrs.iter().any(|attr| attr.path().is_ident("island"))
    })
}

/// Whether the page's own `page` function carries `#[island]`, which the registration cannot call.
pub(crate) fn page_fn_is_island(parsed: &File) -> bool {
    parsed.items.iter().any(|item| {
        let Item::Fn(func) = item else { return false };
        func.sig.ident == "page" && func.attrs.iter().any(|attr| attr.path().is_ident("island"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> File {
        syn::parse_file(src).expect("valid source")
    }

    #[test]
    fn page_with_an_island_component_is_an_island_file() {
        let file = parse("pub fn page() {}\n#[island] fn Counter() {}");
        assert!(is_island(&file));
        assert!(!page_fn_is_island(&file));
    }

    #[test]
    fn plain_page_is_not_an_island() {
        let file = parse("pub fn page() {}");
        assert!(!is_island(&file));
        assert!(!page_fn_is_island(&file));
    }

    #[test]
    fn island_on_the_page_fn_is_detected() {
        let file = parse("#[island] pub fn page() {}");
        assert!(page_fn_is_island(&file));
    }
}
