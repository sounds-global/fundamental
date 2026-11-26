//! A page offers itself in each language it is published in, its own language marked.

use std::sync::Arc;

use fundamental_core::address::{
    AddressSpace, LocalePrefixSet, PageSlug, RequestAddress, SlugTable,
};
use fundamental_core::i18n::Locale;
use fundamental_web::PageContext;

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a language tag")
}

/// English at the bare addresses, Dutch under `/nl` in Dutch words.
fn space() -> Arc<AddressSpace> {
    let prefixes = LocalePrefixSet::new()
        .with("nl", locale("nl"))
        .expect("a prefix");
    let slugs = SlugTable::builder()
        .page("/why", &[PageSlug::current("nl", "waarom")])
        .build()
        .expect("well formed");
    Arc::new(
        AddressSpace::new(prefixes, slugs)
            .with_default_language(locale("en"))
            .expect("a default language"),
    )
}

/// `/why` rendered in Dutch, at `/nl/waarom`.
fn dutch_page() -> PageContext {
    PageContext::new(Vec::new(), "req")
        .with_locale(Some(locale("nl")))
        .with_address(Some(RequestAddress::new(
            space(),
            locale("nl"),
            "nl",
            "/why",
        )))
}

#[test]
fn a_page_offers_itself_at_its_address_in_each_language() {
    let offered: Vec<_> = dutch_page()
        .languages()
        .into_iter()
        .map(|option| {
            (
                option.locale().to_string(),
                option.href().to_owned(),
                option.is_current(),
            )
        })
        .collect();
    assert_eq!(
        offered,
        vec![
            ("en".to_owned(), "/why?lang=en".to_owned(), false),
            ("nl".to_owned(), "/nl/waarom?lang=nl".to_owned(), true),
        ]
    );
}

#[test]
fn a_page_with_no_address_space_offers_no_language() {
    let page = PageContext::new(Vec::new(), "req").with_locale(Some(locale("nl")));
    assert!(page.languages().is_empty());
}
