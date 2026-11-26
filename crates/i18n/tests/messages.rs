//! A word asked for by its id is found in the crate that declares it; an app's own messages win.

use fundamental_core::i18n::{ArgValue, Locale, Translator};
use fundamental_i18n::Catalog;

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a valid locale")
}

#[test]
fn a_crate_s_message_is_found_by_its_id_in_the_reader_s_language() {
    let catalog = Catalog::builder(locale("en"))
        .with_messages(&fundamental_messages_fixture::m::CATALOGS)
        .build();
    let dutch = catalog.translator(&locale("nl"));
    assert_eq!(
        dutch
            .lookup("panel.version", &[("number", ArgValue::Int(2))])
            .as_deref(),
        Some("Versie 2")
    );
    assert!(dutch.carries("panel.version"));
    assert!(!dutch.carries("panel.absent"));
    assert_eq!(dutch.lookup("panel.version", &[]), None);
}

#[test]
fn an_app_s_own_messages_say_a_crate_s_message_its_way() {
    let catalog = Catalog::builder(locale("en"))
        .with_own_messages(&fundamental_messages_app_fixture::m::CATALOGS)
        .with_messages(&fundamental_messages_fixture::m::CATALOGS)
        .build();
    let dutch = catalog.translator(&locale("nl"));
    let shown = fundamental_core::i18n::with(std::sync::Arc::new(dutch), || {
        (
            fundamental_messages_fixture::m::panel::save_for_owner().to_string(),
            fundamental_messages_fixture::m::change::any_value("stage").to_string(),
            fundamental_messages_fixture::m::panel::version(3).to_string(),
        )
    });
    assert_eq!(
        shown,
        (
            "Bewaren voor de eigenaar".to_owned(),
            "Elke fase".to_owned(),
            "Versie 3".to_owned()
        ),
        "the app's words where it has some, the crate's where it has none"
    );
    let english = catalog.translator(&locale("en"));
    assert_eq!(
        english.lookup("panel.save_for_owner", &[]).as_deref(),
        Some("Keep for an owner"),
        "asked for by its id, the app's words win too"
    );
}

#[test]
fn a_language_a_crate_s_messages_speak_is_one_the_page_writes_its_dates_in() {
    let catalog = Catalog::builder(locale("en"))
        .with_messages(&fundamental_messages_fixture::m::CATALOGS)
        .build();
    assert!(catalog.has_locale(&locale("nl")));
    assert!(!catalog.has_locale(&locale("de")));
    let dutch = catalog.date_time_formatter(&locale("nl"), chrono_tz::Europe::Amsterdam);
    assert_eq!(
        dutch.relative(0, fundamental_i18n::RelativeUnit::Day),
        "vandaag"
    );
}

#[test]
fn the_framework_s_own_words_make_no_language_the_app_s() {
    let catalog = Catalog::builder(locale("en")).build();
    assert!(!catalog.has_locale(&locale("nl")));
}
