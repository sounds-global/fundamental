use leptos::prelude::*;

pub use crate::LegacyData as Data;

pub fn page() -> impl IntoView {
    let Data(word) = use_context::<Data>().expect("the page's data is in context");
    view! { <p id="word">{word.0}</p> }
}
