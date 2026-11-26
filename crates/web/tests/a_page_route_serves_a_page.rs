//! A page route says it serves a page, so a sitemap can check that each one is served.
#![cfg(all(feature = "ssr", not(target_arch = "wasm32")))]

use fundamental_web::prelude::*;
use fundamental_web::{PageRoute, PolicyChain, Router};

#[test]
fn a_page_route_serves_a_page() {
    let document = |_: &fundamental_web::PageContext, body: &str| {
        format!("<!doctype html><html><body>{body}</body></html>")
    };
    let router = Router::<()>::new()
        .route(
            PageRoute::new("/prices", PolicyChain::root())
                .expect("page policy")
                .view(|| view! { <h1>"Prices"</h1> }, document),
        )
        .expect("page route registers");
    assert!(router.serves_page("/prices"));
    assert!(!router.serves_page("/elsewhere"));
}
