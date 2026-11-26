//! A crate's messages are said in the language asked for.

use fundamental_messages_fixture::m;

#[test]
fn a_message_is_said_in_the_language_asked_for() {
    assert_eq!(
        m::panel::save_for_owner().format_in("en"),
        "Save for an owner"
    );
    assert_eq!(
        m::panel::save_for_owner().format_in("nl"),
        "Opslaan voor een eigenaar"
    );
    assert_eq!(m::panel::version(2).format_in("nl"), "Versie 2");
}

#[test]
fn a_count_takes_the_plural_its_language_asks() {
    assert_eq!(m::panel::pages(1).format_in("en"), "1 page");
    assert_eq!(m::panel::pages(3).format_in("en"), "3 pages");
    assert_eq!(m::panel::pages(1).format_in("nl"), "1 pagina");
    assert_eq!(m::panel::pages(3).format_in("nl"), "3 pagina's");
}

#[test]
fn a_word_is_chosen_by_the_text_it_is_given() {
    assert_eq!(m::panel::filter("stage").format_in("en"), "Any stage");
    assert_eq!(m::panel::filter("kind").format_in("en"), "Any");
    assert_eq!(m::panel::filter("stage").format_in("nl"), "Elke fase");
}

#[test]
fn a_language_the_crate_does_not_have_says_the_source() {
    assert_eq!(m::panel::greeting("Ada").format_in("nl"), "Hallo Ada");
    assert_eq!(m::panel::version(2).format_in("de"), "Version 2");
}

#[test]
fn a_message_becomes_the_string_a_prop_takes() {
    let words: String = m::panel::save_for_owner().into();
    assert_eq!(words, "Save for an owner");
}

#[test]
fn a_notice_s_mail_is_said_with_its_scope() {
    assert_eq!(
        m::notice::message_subject("Plan").format_in("nl"),
        "Nieuwe berichten in Plan"
    );
    assert_eq!(
        m::notice::blocking_subject("Plan").format_in("en"),
        "A question is waiting in Plan"
    );
    assert_eq!(
        m::notice::sign_in_subject().format_in("nl"),
        "Je aanmelding"
    );
    assert_eq!(m::notice::body("Two new").format_in("en"), "Two new");
}

#[test]
fn a_message_said_only_in_a_language_borrows_no_other_language_s_words() {
    let save = || m::panel::save_for_owner();
    assert_eq!(
        save().format_only_in("nl").as_deref(),
        Some("Opslaan voor een eigenaar")
    );
    // A region is its language's.
    assert_eq!(
        save().format_only_in("nl-BE").as_deref(),
        Some("Opslaan voor een eigenaar")
    );
    assert_eq!(
        save().format_only_in("en-GB").as_deref(),
        Some("Save for an owner")
    );
    // A language the crate has no words in is said in none, never in the source's.
    assert_eq!(save().format_only_in("de"), None);
}
