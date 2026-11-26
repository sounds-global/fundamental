use leptos::prelude::*;

/// Renders `view_fn`'s view to an HTML string under a fresh reactive [`Owner`], synchronously.
///
/// ```
/// # extern crate fundamental_web;
/// use fundamental_web::prelude::*;
/// use fundamental_web::render_to_html;
///
/// let html = render_to_html(|| view! { <h1>"Hello, world"</h1> });
/// assert!(html.contains("Hello, world"));
/// ```
pub fn render_to_html<F, V>(view_fn: F) -> String
where
    F: FnOnce() -> V,
    V: RenderHtml + 'static,
{
    let owner = Owner::new();
    owner.with(|| view_fn().to_html())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_static_view_to_markup() {
        let html = render_to_html(|| view! { <h1>"Hello, world"</h1> });
        assert!(html.contains("<h1>"), "rendered: {html}");
        assert!(html.contains("Hello, world"), "rendered: {html}");
    }

    #[test]
    fn renders_a_component_with_a_signal() {
        #[component]
        fn Counter(start: i32) -> impl IntoView {
            let (count, _set) = signal(start);
            view! { <span>"count: "{move || count.get()}</span> }
        }

        let html = render_to_html(|| view! { <Counter start=41 /> });
        assert!(html.contains("count: "), "rendered: {html}");
        assert!(html.contains("41"), "rendered: {html}");
    }
}
