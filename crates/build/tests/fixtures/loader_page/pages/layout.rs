use leptos::prelude::*;

pub fn layout(children: impl IntoView + 'static) -> impl IntoView {
    view! { <main>{children}</main> }
}
