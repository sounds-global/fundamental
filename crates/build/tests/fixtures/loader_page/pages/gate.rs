use fundamental::web::Refused;
use leptos::prelude::*;

pub fn page(refused: Refused) -> impl IntoView {
    let status = refused.status();
    view! { <h1>{status}</h1> }
}
