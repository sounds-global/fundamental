use fundamental_server::HttpContext;
use leptos::prelude::*;

use crate::render_options::RenderOptions;
use crate::render_page::render_with;
use crate::rendered_page::RenderedPage;

/// Renders `view_fn`'s view with `data` in context, alongside everything [`render_page`](fn@crate::render_page) provides.
pub fn render_page_with<S, D, F, V>(ctx: &HttpContext<S>, data: D, view_fn: F) -> RenderedPage
where
    S: Clone + Send + Sync + 'static,
    D: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    render_with(
        ctx,
        RenderOptions::default(),
        move || provide_context(data),
        view_fn,
    )
}

#[cfg(test)]
mod tests {
    use super::render_page_with;
    use fundamental_server::HttpContext;
    use leptos::prelude::*;

    #[derive(Clone)]
    struct AppState;

    #[derive(Clone)]
    struct Data {
        heading: &'static str,
    }

    fn context() -> HttpContext<AppState> {
        HttpContext::new(AppState, "req-1")
    }

    #[test]
    fn a_page_reads_its_data_back_from_context() {
        let rendered = render_page_with(&context(), Data { heading: "Devices" }, || {
            let data = use_context::<Data>().expect("the page's data is in context");
            view! { <h1>{data.heading}</h1> }
        });
        assert!(rendered.body().contains("Devices"));
    }

    #[test]
    fn the_data_does_not_displace_what_every_page_already_reads() {
        let rendered = render_page_with(&context(), Data { heading: "Devices" }, || {
            let has_state = use_context::<AppState>().is_some();
            let has_context = use_context::<crate::PageContext>().is_some();
            view! { <p>{format!("{has_state} {has_context}")}</p> }
        });
        assert!(rendered.body().contains("true true"));
    }
}
