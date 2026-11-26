//! A record type's words are its crate's messages.

use fundamental_core::Variant as _;
use fundamental_messages::{FieldWords, Messages, Noun};
use fundamental_messages_fixture::m;

/// What an article reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, fundamental_macros::Variant, Messages)]
#[messages(m::kind)]
enum Kind {
    Milestone,
    ImportantChange,
}

/// The articles a team keeps.
#[derive(Messages)]
#[messages(m::model::article)]
struct Articles;

/// What a team changes of an article.
#[derive(Messages)]
#[messages(m::model::article::field)]
#[expect(dead_code, reason = "only the fields' names and types are read")]
struct ArticleEdit {
    headline: String,
    #[messages(variant)]
    kind: Kind,
}

#[test]
fn a_type_is_named_in_each_form_in_the_reader_s_language() {
    assert_eq!(Articles::one().format_in("en"), "article");
    assert_eq!(Articles::many().format_in("nl"), "artikelen");
    assert_eq!(Articles::the().format_in("nl"), "het artikel");
    assert_eq!(Articles::new_one().format_in("nl"), "nieuw artikel");
    assert_eq!(Articles::this().format_in("en"), "this article");
}

#[test]
fn each_field_is_said_by_the_message_of_its_name() {
    let label = |field| ArticleEdit::field(field).map(|message| message.format_in("nl"));
    assert_eq!(label("headline").as_deref(), Some("Kop"));
    assert_eq!(label("kind").as_deref(), Some("Soort"));
    assert_eq!(label("body"), None, "a name that is no field has none");
}

#[test]
fn a_closed_field_s_value_is_said_by_its_variant_s_message() {
    let said =
        |field, value| ArticleEdit::value(field, value).map(|message| message.format_in("nl"));
    assert_eq!(
        said("kind", Kind::ImportantChange.value()).as_deref(),
        Some("Belangrijke wijziging")
    );
    assert_eq!(said("kind", "mauve"), None, "a value no variant has");
    assert_eq!(
        said("headline", "x"),
        None,
        "an open field's value is its own"
    );
}
