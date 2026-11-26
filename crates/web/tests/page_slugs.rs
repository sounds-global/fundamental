//! The `slugs!` declaration a page file carries, end to end.

#![cfg(not(target_arch = "wasm32"))]

use fundamental_core::address::PagePath;
use fundamental_web::{AddressSpace, LocalePrefixSet, PageSlug, SlugTable};

mod why {
    fundamental_macros::slugs! {
        nl = "waarom",
        sv = "varfor",
        retired nl = "waarom-acme",
    }

    pub fn page() -> &'static str {
        "why"
    }
}

mod guides {
    fundamental_macros::slugs! {
        nl = "gidsen",
    }
}

mod trust {
    /// What a page with no `slugs!` contributes to the table.
    pub const PAGE_SLUGS: &[super::PageSlug] = &[];
}

#[test]
fn the_macro_expands_to_the_constant_the_registration_names() {
    assert_eq!(why::PAGE_SLUGS.len(), 3);
    assert_eq!(why::PAGE_SLUGS[0].language(), "nl");
    assert_eq!(why::PAGE_SLUGS[0].slug(), "waarom");
    assert!(why::PAGE_SLUGS[0].is_current());
    assert_eq!(why::PAGE_SLUGS[1].slug(), "varfor");
    assert!(
        !why::PAGE_SLUGS[2].is_current(),
        "a retired spelling must not be emitted as an address"
    );
    let _ = why::page();
    assert!(trust::PAGE_SLUGS.is_empty());
}

#[test]
fn the_declared_slugs_become_the_pages_addresses() {
    let slugs = SlugTable::builder()
        .page("/why", why::PAGE_SLUGS)
        .page("/guides", guides::PAGE_SLUGS)
        .page("/trust", trust::PAGE_SLUGS)
        .build()
        .expect("the declarations are unambiguous");

    let prefixes = LocalePrefixSet::new()
        .with("nl", "nl-NL".parse().expect("tag"))
        .expect("nl")
        .with("se", "sv-SE".parse().expect("tag"))
        .expect("se");
    let space = AddressSpace::new(prefixes, slugs);

    assert_eq!(space.address("nl", "/why"), "/nl/waarom");
    assert_eq!(space.address("se", "/why"), "/se/varfor");
    assert_eq!(space.address("nl", "/trust"), "/nl/trust");

    let resolved = space.resolve("/nl/waarom", None);
    assert_eq!(resolved.canonical_path(), "/why");
    assert_eq!(resolved.redirect_target(), None);

    // The retired spelling still resolves, permanently redirecting to the current one.
    let retired = space.resolve("/nl/waarom-acme", None);
    assert_eq!(retired.canonical_path(), "/why");
    assert_eq!(retired.redirect_target(), Some("/nl/waarom"));
}

#[test]
fn a_directory_slug_moves_every_page_beneath_it() {
    let slugs = SlugTable::builder()
        .page("/guides", guides::PAGE_SLUGS)
        .page("/guides/api", trust::PAGE_SLUGS)
        .build()
        .expect("unambiguous");
    let prefixes = LocalePrefixSet::new()
        .with("nl", "nl-NL".parse().expect("tag"))
        .expect("nl");
    let space = AddressSpace::new(prefixes, slugs);

    assert_eq!(
        space.address("nl", "/guides/api"),
        "/nl/gidsen/api",
        "the page beneath it never mentions the directory"
    );
    assert_eq!(
        space.resolve("/nl/gidsen/api", None).canonical_path(),
        "/guides/api"
    );
}

#[test]
fn a_page_builds_its_links_and_alternates_from_its_address() {
    use fundamental_web::{PageContext, RequestAddress};
    use std::sync::Arc;

    let slugs = SlugTable::builder()
        .page("/why", why::PAGE_SLUGS)
        .page("/trust", trust::PAGE_SLUGS)
        .build()
        .expect("unambiguous");
    let prefixes = LocalePrefixSet::new()
        .with("nl", "nl-NL".parse().expect("tag"))
        .expect("nl")
        .with("se", "sv-SE".parse().expect("tag"))
        .expect("se");
    let space = Arc::new(AddressSpace::new(prefixes, slugs));

    let ctx = PageContext::new(Vec::new(), "req-1").with_address(Some(RequestAddress::new(
        Arc::clone(&space),
        "nl-NL".parse().expect("tag"),
        "/nl",
        "/why",
    )));

    assert_eq!(ctx.href_to(&PagePath::of_route("/trust")), "/nl/trust");
    assert_eq!(ctx.href_to(&PagePath::of_route("/why")), "/nl/waarom");

    for (hreflang, href) in ctx.address().expect("addressed").alternates() {
        ctx.add_alternate(hreflang, href);
    }
    let html = ctx.head().render();
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="nl-NL" href="/nl/waarom">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<link rel="alternate" hreflang="sv-SE" href="/se/varfor">"#),
        "{html}"
    );
    assert!(html.contains(r#"hreflang="x-default" href="/""#), "{html}");
}

#[test]
fn a_page_prints_as_its_route_path_without_an_address_space() {
    use fundamental_web::PageContext;

    let ctx = PageContext::new(Vec::new(), "req-1");
    assert!(ctx.address().is_none());
    assert_eq!(ctx.href_to(&PagePath::of_route("/why")), "/why");
}

/// A Dutch-authored tree: its own spelling and the words of the steps at `/aanvraag/{step}`.
mod build_request {
    fundamental_macros::slugs! {
        en = "build-request",
        beneath "bedrijf" { en = "company" },
        beneath "gebruikers" { en = "users", retired en = "people" },
    }
}

#[test]
fn the_words_beneath_a_page_are_spelled_from_its_own_declaration() {
    assert_eq!(build_request::WORD_SLUGS.len(), 2);
    assert!(
        why::WORD_SLUGS.is_empty(),
        "a page with no words declares none"
    );
    let slugs = SlugTable::builder()
        .page("/aanvraag", build_request::PAGE_SLUGS)
        .with_segments("/aanvraag", build_request::WORD_SLUGS)
        .page("/aanvraag/{step}", &[])
        .build()
        .expect("the declarations are unambiguous");
    let prefixes = LocalePrefixSet::new()
        .with("en", "en-GB".parse().expect("tag"))
        .expect("en");
    let space = AddressSpace::new(prefixes, slugs);

    assert_eq!(
        space.address("en", "/aanvraag/bedrijf"),
        "/en/build-request/company"
    );
    assert_eq!(
        space.address("en", "/aanvraag/app"),
        "/en/build-request/app"
    );
    let resolved = space.resolve("/en/build-request/users", None);
    assert_eq!(resolved.canonical_path(), "/aanvraag/gebruikers");
    assert_eq!(resolved.redirect_target(), None);
    assert_eq!(
        space
            .resolve("/en/build-request/people", None)
            .redirect_target(),
        Some("/en/build-request/users"),
        "a retired word keeps answering, at its current spelling"
    );
}
