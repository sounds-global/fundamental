use leptos::prelude::*;

use crate::Overview;

pub fn page(model: Overview) -> impl IntoView {
    view! { <h1>{model.name}</h1> }
}
