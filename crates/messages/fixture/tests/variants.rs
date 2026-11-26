//! A closed value says itself through its messages.

use fundamental_messages::Messages;
use fundamental_messages_fixture::m;

/// What an article reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Messages)]
#[messages(m::kind)]
enum Kind {
    Milestone,
    ImportantChange,
}

#[test]
fn each_variant_is_said_by_the_message_of_its_name() {
    assert_eq!(Kind::Milestone.message().format_in("en"), "Milestone");
    assert_eq!(
        Kind::ImportantChange.message().format_in("nl"),
        "Belangrijke wijziging"
    );
    assert_eq!(
        Kind::ImportantChange.message().id(),
        "kind.important_change"
    );
}

#[test]
fn a_variant_shows_as_its_words() {
    assert_eq!(Kind::ImportantChange.to_string(), "Important change");
}

/// What happened to a record, with what each message says it with.
#[derive(Clone, Debug, PartialEq, Eq, Messages)]
#[messages(m::change)]
enum Change {
    ChangedAt { at: String },
    Counted { count: u32, noun: String },
    AnyValue { field: String },
}

#[test]
fn a_variant_s_fields_are_its_message_s_inputs() {
    let counted = Change::Counted {
        count: 1200,
        noun: "artikelen".to_owned(),
    };
    assert_eq!(counted.message().format_in("nl"), "1.200 artikelen");
    assert_eq!(
        Change::ChangedAt {
            at: "10:00".to_owned()
        }
        .message()
        .format_in("nl"),
        "gewijzigd 10:00"
    );
    assert_eq!(
        Change::AnyValue {
            field: "kind".to_owned()
        }
        .to_string(),
        "All"
    );
}

/// An act, one named as a Rust keyword.
#[derive(Clone, Copy, Debug, Messages)]
#[messages(m::act)]
enum Act {
    Do,
}

#[test]
fn a_variant_named_as_a_rust_keyword_calls_its_raw_name() {
    assert_eq!(Act::Do.message().format_in("nl"), "Doen");
}
