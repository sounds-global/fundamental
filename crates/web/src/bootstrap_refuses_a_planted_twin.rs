//! The island bootstrap refuses a page it could be steered on, in a real browser.
//!
//! Run with:
//!   wasm-pack test --headless --firefox crates/web --no-default-features --features hydrate

use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const BOOTSTRAP: &str = include_str!("hydration_bootstrap.js");

fn document() -> web_sys::Document {
    web_sys::window()
        .and_then(|window| window.document())
        .expect("a browser document")
}

/// Plants `count` hydration elements pointing at `bundle`, runs the bootstrap, and answers its state.
fn run_with(count: usize, bundle: &str) -> Option<String> {
    let document = document();
    let body = document.body().expect("a body");
    // The test runner reports into this body too.
    let planted = document
        .query_selector_all("[data-fundamental-hydration]")
        .expect("a selector");
    for at in 0..planted.length() {
        if let Some(element) = planted.item(at) {
            body.remove_child(&element).expect("removed");
        }
    }
    for _ in 0..count {
        let script = document.create_element("script").expect("an element");
        script.set_attribute("type", "text/plain").expect("type");
        script
            .set_attribute("data-fundamental-hydration", "")
            .expect("marker");
        script.set_attribute("data-bundle", bundle).expect("bundle");
        script
            .set_attribute("data-wasm", "/_fundamental/app_bg.wasm")
            .expect("wasm");
        body.append_child(&script).expect("appended");
    }
    js_sys::eval(BOOTSTRAP).expect("the bootstrap runs");
    document
        .document_element()
        .and_then(|root| root.get_attribute("data-hydration"))
}

#[wasm_bindgen_test]
fn a_second_hydration_element_is_refused() {
    assert_eq!(
        run_with(2, "/_fundamental/app.js").as_deref(),
        Some("failed")
    );
}

#[wasm_bindgen_test]
fn a_bundle_on_another_origin_is_refused() {
    assert_eq!(
        run_with(1, "https://evil.example/app.js").as_deref(),
        Some("failed")
    );
    assert_eq!(
        run_with(1, "//evil.example/app.js").as_deref(),
        Some("failed")
    );
}

#[wasm_bindgen_test]
fn one_element_on_this_origin_goes_on_to_load_its_bundle() {
    assert_eq!(
        run_with(1, "/_fundamental/app.js").as_deref(),
        Some("pending"),
        "the refusal is for a steered page only; the real one starts loading"
    );
}
