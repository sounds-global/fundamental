//! A snapshot holds what a person reviews: the markup, never what changes from build to build.

use fundamental_test_render::{assert_html_snapshot, normalize_html, render_html};
use leptos::prelude::*;

#[test]
fn an_islands_build_hash_is_masked() {
    assert_eq!(
        normalize_html(
            r#"<leptos-island data-component="Disclosure_4989255541276399206"></leptos-island>"#
        ),
        "<leptos-island data-component=\"Disclosure_[hash]\">\n</leptos-island>"
    );
}

#[test]
fn a_name_with_underscores_keeps_them() {
    assert_eq!(
        normalize_html(r#"<x data-component="Tab_bar_12"></x>"#),
        "<x data-component=\"Tab_bar_[hash]\">\n</x>"
    );
}

#[test]
fn a_component_value_without_a_hash_is_left_alone() {
    assert_eq!(
        normalize_html(r#"<x data-component="menu"></x>"#),
        "<x data-component=\"menu\">\n</x>"
    );
}

#[test]
fn the_renderers_placeholder_markers_are_dropped() {
    assert_eq!(normalize_html("<p>a<!>b</p>"), "<p>ab</p>");
}

#[test]
fn each_element_starts_its_own_line() {
    assert_eq!(
        normalize_html("<ul><li>a</li><li>b</li></ul>"),
        "<ul>\n<li>a</li>\n<li>b</li>\n</ul>"
    );
}

#[island]
fn Disclosure(children: Children) -> impl IntoView {
    view! { <details>{children()}</details> }
}

#[test]
fn an_island_snapshot_is_stable() {
    assert_html_snapshot!(render_html(
        || view! { <Disclosure><p>"Inhoud"</p></Disclosure> }
    ));
}
