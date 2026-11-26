//! A streamed page names its canonical URL and its `hreflang` alternates in its head.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use std::sync::Arc;

use fundamental_core::address::{AddressSpace, LocalePrefixSet, SlugTable, WebOrigin};
use fundamental_core::i18n::Locale;
use fundamental_server::{Method, ServerRequest};
use fundamental_web::prelude::*;
use fundamental_web::{PageContext, PageRoute, PolicyChain, Router};

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a language tag")
}

fn document(page: &PageContext, body: &str) -> String {
    format!(
        "<!doctype html><html><head>{}</head><body>{body}</body></html>",
        page.head().render()
    )
}

#[tokio::test]
async fn a_streamed_page_names_its_canonical_url_and_alternates_in_its_head() {
    let prefixes = LocalePrefixSet::new().with("nl", locale("nl")).expect("nl");
    let space = AddressSpace::new(prefixes, SlugTable::default())
        .with_default_language(locale("en"))
        .expect("en")
        .with_origin(WebOrigin::parse("https://site.example").expect("origin"));
    let router = Router::<()>::new()
        .route(
            PageRoute::new("/prices", PolicyChain::root())
                .expect("page policy")
                .view(|| view! { <h1>"Prices"</h1> }, document),
        )
        .expect("page route registers")
        .with_addresses(Arc::new(space));
    let response = router
        .dispatch(ServerRequest::new(Method::Get, "/prices", ()))
        .await
        .buffered()
        .await;
    assert_eq!(response.status(), 200);
    let html = String::from_utf8_lossy(response.body()).into_owned();
    let head = &html[..html.find("</head>").expect("a head")];
    assert!(
        head.contains(r#"<link rel="canonical" href="https://site.example/prices">"#),
        "{head}"
    );
    assert!(
        head.contains(r#"hreflang="nl" href="https://site.example/nl/prices""#),
        "{head}"
    );
}
