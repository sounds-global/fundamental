//! In-browser proof that an `#[island]` is interactive.
//!
//! Run with:
//!   wasm-pack test --headless --firefox crates/web --no-default-features --features hydrate

#![cfg(target_arch = "wasm32")]

use fundamental_web::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[island]
fn Counter(start: i32) -> impl IntoView {
    let (count, set) = signal(start);
    view! {
        <button on:click=move |_| set.update(|n| *n += 1)>
            {move || count.get().to_string()}
        </button>
    }
}

#[wasm_bindgen_test]
async fn island_increments_on_click() {
    let document = web_sys::window().unwrap().document().unwrap();
    let host = document.create_element("div").unwrap();
    document.body().unwrap().append_child(&host).unwrap();

    let host_el: web_sys::HtmlElement = host.clone().dyn_into().unwrap();
    leptos::mount::mount_to(host_el, || view! { <Counter start=0 /> }).forget();

    let button = host
        .query_selector("button")
        .unwrap()
        .expect("the island mounted a button");
    assert_eq!(button.text_content().unwrap_or_default(), "0");

    let click = web_sys::MouseEvent::new("click").unwrap();
    button.dispatch_event(&click).unwrap();

    // Leptos flushes the reactive DOM update on a microtask.
    yield_to_microtasks().await;
    assert_eq!(button.text_content().unwrap_or_default(), "1");
}

async fn yield_to_microtasks() {
    let promise = js_sys::Promise::resolve(&wasm_bindgen::JsValue::NULL);
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[wasm_bindgen_test]
fn prefetch_links_injects_a_prefetch_link_on_hover() {
    let document = web_sys::window().unwrap().document().unwrap();
    let anchor = document.create_element("a").unwrap();
    anchor.set_attribute("href", "/about").unwrap();
    document.body().unwrap().append_child(&anchor).unwrap();

    fundamental_web::prefetch_links();

    let init = web_sys::EventInit::new();
    init.set_bubbles(true);
    let event = web_sys::Event::new_with_event_init_dict("pointerover", &init).unwrap();
    anchor.dispatch_event(&event).unwrap();

    let link = document
        .query_selector("link[rel=\"prefetch\"]")
        .unwrap()
        .expect("a prefetch link was injected");
    assert!(
        link.get_attribute("href")
            .unwrap_or_default()
            .contains("/about"),
        "prefetch link points at the hovered target"
    );

    anchor.dispatch_event(&event).unwrap();
    let count = document
        .query_selector_all("link[rel=\"prefetch\"]")
        .unwrap()
        .length();
    assert_eq!(count, 1, "prefetch is injected once per URL");
}

#[wasm_bindgen_test]
fn scroll_to_top_runs_without_panicking() {
    fundamental_web::scroll_to_top();
}
