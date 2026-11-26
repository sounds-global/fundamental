//! A page addressed by a declared segment names the entity's canonical URL only once authorization affirmed it.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use fundamental_core::AnyError;
use fundamental_core::address::{AddressSpace, LocalePrefixSet, SlugTable, WebOrigin};
use fundamental_core::i18n::Locale;
use fundamental_server::{
    HttpContext, Method, ResolvedSegment, Route, Router, SegmentResolution, SegmentResolver,
    ServerRequest,
};
use fundamental_web::render_page;
use leptos::prelude::*;

#[derive(Clone)]
struct State;

struct ProductSlugs;

impl SegmentResolver for ProductSlugs {
    fn param(&self) -> &str {
        "product"
    }

    fn void_id(&self) -> String {
        "void".to_owned()
    }

    async fn resolve<'a>(
        &'a self,
        raw: &'a str,
        _scope: Option<&'a ResolvedSegment>,
        _locale: Option<&'a Locale>,
    ) -> Result<SegmentResolution, AnyError> {
        let product = ResolvedSegment::new("p-1", "eiken-tafel")
            .and_then(|segment| segment.with_spelling(locale("en"), "oak-table"));
        Ok(match (raw, product) {
            ("eiken-tafel" | "oak-table", Some(product)) => SegmentResolution::Canonical(product),
            _ => SegmentResolution::Unknown,
        })
    }
}

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a language tag")
}

#[derive(Clone, Copy)]
enum Loader {
    /// Anyone may see the product.
    Affirms,
    /// Nothing affirmed it.
    Silent,
    /// It affirmed the product, then refused the page with a `404`.
    AffirmsThenRefuses,
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn router(loader: Loader) -> Router<State> {
    let prefixes = LocalePrefixSet::new().with("en", locale("en")).expect("en");
    let space = AddressSpace::new(prefixes, SlugTable::default())
        .with_default_language(locale("nl"))
        .expect("nl")
        .with_origin(WebOrigin::parse("https://shop.example").expect("origin"));
    Router::new()
        .with_addresses(Arc::new(space))
        .segment(ProductSlugs)
        .and_then(|router| {
            router.route(Route::html(
                "/producten/{product}",
                move |ctx: HttpContext<State>| async move {
                    match loader {
                        Loader::Affirms => ctx.affirm("product"),
                        Loader::Silent => {}
                        Loader::AffirmsThenRefuses => {
                            ctx.affirm("product");
                            ctx.set_response_status(404);
                        }
                    }
                    let rendered = render_page(&ctx, || {
                        if let Some(page) = use_context::<fundamental_web::PageContext>() {
                            let mut fields = serde_json::Map::new();
                            fields.insert("name".to_owned(), "Eiken tafel".into());
                            page.add_structured_data_about("Product", fields);
                        }
                        view! { <p>"tafel"</p> }
                    });
                    let page = rendered.context();
                    let head = page.head();
                    let paths = page
                        .address()
                        .map(|address| (address.canonical_url(), address.alternate_urls()));
                    Ok(format!(
                        "{:?} {:?} {:?} {paths:?}",
                        head.canonical, head.alternates, head.structured_data
                    ))
                },
            ))
        })
        .expect("a well-declared router")
}

async fn head(router: &Router<State>, path: &str) -> String {
    answer(router, path, 200).await
}

async fn answer(router: &Router<State>, path: &str, status: u16) -> String {
    let response = router
        .dispatch(ServerRequest::new(Method::Get, path, State))
        .await;
    assert_eq!(response.status(), status, "{path}");
    String::from_utf8(response.body().to_vec()).expect("utf-8")
}

#[tokio::test]
async fn the_head_names_the_entity_canonical_and_its_alternates() {
    let english = head(&router(Loader::Affirms), "/en/producten/oak-table").await;
    assert!(
        english.starts_with("Some(\"https://shop.example/en/producten/oak-table\")"),
        "{english}"
    );
    assert!(
        english.contains("(\"nl\", \"https://shop.example/producten/eiken-tafel\")"),
        "{english}"
    );
    assert!(
        english.contains("(\"en\", \"https://shop.example/en/producten/oak-table\")"),
        "{english}"
    );
    assert!(
        english.contains(r#"\"@id\":\"https://shop.example/en/producten/oak-table\""#),
        "the structured data names the canonical URL: {english}"
    );
    let dutch = head(&router(Loader::Affirms), "/producten/eiken-tafel").await;
    assert!(
        dutch.starts_with("Some(\"https://shop.example/producten/eiken-tafel\")"),
        "{dutch}"
    );
}

#[tokio::test]
async fn an_unaffirmed_entity_names_nothing_in_its_head() {
    let unaffirmed = head(&router(Loader::Silent), "/en/producten/oak-table").await;
    assert!(unaffirmed.starts_with("None []"), "{unaffirmed}");
    assert!(!unaffirmed.contains("oak-table\\\""), "{unaffirmed}");
}

#[tokio::test]
async fn a_refused_page_names_no_address() {
    let refused = answer(
        &router(Loader::AffirmsThenRefuses),
        "/en/producten/oak-table",
        404,
    )
    .await;
    assert!(
        refused.starts_with("None []"),
        "no canonical, no alternates: {refused}"
    );
    assert!(
        refused.ends_with("Some((None, None))"),
        "the address the page renders with names nothing either: {refused}"
    );
    assert!(
        !refused.contains("eiken-tafel"),
        "no other spelling leaks: {refused}"
    );
}
