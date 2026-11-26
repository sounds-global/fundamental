//! A message renders in a view and in an attribute in the page's language.

use std::sync::Arc;

use fundamental_core::i18n::{Arg, Translator, with};
use fundamental_messages_fixture::m;
use fundamental_test_render::render_html;
use leptos::prelude::*;

/// A page read in Dutch.
struct Dutch;

impl Translator for Dutch {
    fn lookup(&self, _key: &str, _args: &[Arg<'_>]) -> Option<String> {
        None
    }

    fn locale_tag(&self) -> &str {
        "nl"
    }
}

#[test]
fn a_message_renders_in_the_page_s_language() {
    let html = with(Arc::new(Dutch), || {
        render_html(|| {
            view! {
                <p>{m::panel::version(2)}</p>
                <button title=m::panel::save_for_owner()>{m::panel::pages(3)}</button>
            }
        })
    });
    assert!(html.contains("<p>Versie 2</p>"), "{html}");
    assert!(
        html.contains(r#"title="Opslaan voor een eigenaar""#),
        "{html}"
    );
    assert!(
        html.contains("3 pagina&#x27;s") || html.contains("3 pagina's"),
        "{html}"
    );
}
