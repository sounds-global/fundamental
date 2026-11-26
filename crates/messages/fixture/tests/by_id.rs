//! A crate's catalogs say a message by its id, or nothing so the caller's own words answer.

use fundamental_messages::Value;
use fundamental_messages_fixture::m;

#[test]
fn a_message_is_said_by_its_id_in_the_language_asked_for() {
    assert_eq!(
        m::CATALOGS
            .say("panel.version", "nl", &[("number", Value::Integer(2))])
            .as_deref(),
        Some("Versie 2")
    );
    assert_eq!(
        m::CATALOGS
            .say("panel.save_for_owner", "en-GB", &[])
            .as_deref(),
        Some("Save for an owner")
    );
}

#[test]
fn an_unknown_id_says_nothing() {
    assert_eq!(m::CATALOGS.say("panel.absent", "en", &[]), None);
    assert!(!m::CATALOGS.carries("panel.absent"));
    assert!(m::CATALOGS.carries("panel.version"));
}

#[test]
fn a_message_without_the_value_it_needs_says_nothing() {
    assert_eq!(m::CATALOGS.say("panel.version", "nl", &[]), None);
}
