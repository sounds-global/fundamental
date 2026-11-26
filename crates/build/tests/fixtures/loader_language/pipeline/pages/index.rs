use leptos::prelude::*;

use crate::Word;

pub fn page(word: Word) -> impl IntoView {
    view! { <p id="word">{word.0}</p> }
}
