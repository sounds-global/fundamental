use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{Element, Event, HtmlAnchorElement, window};

/// Prefetches same-origin page links on hover, so the following full-page navigation loads from cache.
pub fn prefetch_links() {
    let Some(document) = window().and_then(|w| w.document()) else {
        return;
    };

    let handler = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if let Some(href) = same_origin_anchor_href(&event) {
            prefetch_once(&href);
        }
    });

    let _ =
        document.add_event_listener_with_callback("pointerover", handler.as_ref().unchecked_ref());
    // The listener lives for the page's lifetime.
    handler.forget();
}

/// The absolute href of the same-origin `<a>` the `event` targets, if any.
fn same_origin_anchor_href(event: &Event) -> Option<String> {
    let element = event.target()?.dyn_into::<Element>().ok()?;
    let anchor = element
        .closest("a[href]")
        .ok()??
        .dyn_into::<HtmlAnchorElement>()
        .ok()?;
    let href = anchor.href();
    let origin = window()?.location().origin().ok()?;
    href.starts_with(&origin).then_some(href)
}

/// Injects a `<link rel="prefetch" href=...>` for `href`, unless one already exists.
fn prefetch_once(href: &str) {
    let Some(document) = window().and_then(|w| w.document()) else {
        return;
    };
    let selector = format!("link[rel=\"prefetch\"][href=\"{href}\"]");
    if matches!(document.query_selector(&selector), Ok(Some(_))) {
        return;
    }
    let Some(head) = document.head() else {
        return;
    };
    let Ok(link) = document.create_element("link") else {
        return;
    };
    let _ = link.set_attribute("rel", "prefetch");
    let _ = link.set_attribute("href", href);
    let _ = head.append_child(link.unchecked_ref());
}
