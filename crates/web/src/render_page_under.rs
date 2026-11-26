use fundamental_server::HttpContext;
use leptos::prelude::*;

use crate::page::PageTitle;
use crate::render_options::RenderOptions;
use crate::render_page::render_with;
use crate::rendered_page::RenderedPage;

/// Renders `view_fn`'s view with the request's [`PageContext`](crate::PageContext) and without the application state.
pub fn render_page_under<S, F, V>(ctx: &HttpContext<S>, view_fn: F) -> RenderedPage
where
    S: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    render_titled_under(ctx, None, view_fn)
}

/// [`render_page_under`] with a policy's title set first.
pub(crate) fn render_titled_under<S, F, V>(
    ctx: &HttpContext<S>,
    title: Option<PageTitle>,
    view_fn: F,
) -> RenderedPage
where
    S: Clone + Send + Sync + 'static,
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    render_with(
        ctx,
        RenderOptions {
            without_state: true,
            title,
        },
        || {},
        view_fn,
    )
}

#[cfg(test)]
mod tests {
    use super::render_page_under;
    use crate::PageContext;
    use fundamental_server::HttpContext;
    use leptos::prelude::*;

    #[derive(Clone)]
    struct AppState;

    #[test]
    fn a_page_rendered_under_has_no_state_in_context() {
        let ctx = HttpContext::new(AppState, "req-under");
        let rendered = render_page_under(&ctx, || {
            let state = use_context::<AppState>().is_some();
            let page = use_context::<PageContext>().is_some();
            view! { <p>{format!("state={state} page={page}")}</p> }
        });
        assert!(
            rendered.body().contains("state=false page=true"),
            "{}",
            rendered.body()
        );
    }

    #[test]
    fn the_model_reaches_page_as_its_argument() {
        struct Overview {
            name: &'static str,
        }
        fn page(model: Overview) -> impl IntoView {
            view! { <h1>{model.name}</h1> }
        }
        let ctx = HttpContext::new(AppState, "req-model");
        let model = Overview { name: "Sounds" };
        let rendered = render_page_under(&ctx, move || page(model));
        assert!(
            rendered.body().contains("<h1>Sounds</h1>"),
            "{}",
            rendered.body()
        );
    }
}
