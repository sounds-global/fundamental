//! `render_html` renders what a served page would: islands with their children included.

use fundamental_test_render::render_html;
use leptos::prelude::*;

#[island]
fn Disclosure(children: Children) -> impl IntoView {
    view! { <details>{children()}</details> }
}

#[test]
fn an_island_renders_its_children_as_a_served_page_does() {
    let html = render_html(|| view! { <Disclosure><p>"Inhoud"</p></Disclosure> });
    assert!(html.contains("<details"), "{html}");
    assert!(html.contains("<p>Inhoud</p>"), "{html}");
}

#[test]
fn a_plain_view_renders_unchanged() {
    assert_eq!(
        render_html(|| view! { <a href="/x">"Ga"</a> }),
        r#"<a href="/x">Ga</a>"#
    );
}
