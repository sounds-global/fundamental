//! Every validation error is said in the reader's language by the framework's own messages.

use std::path::Path;
use std::sync::Arc;

use fundamental_core::i18n::{Locale, with};
use fundamental_core::validate::ValidationError;
use fundamental_i18n::{Catalog, m};

#[test]
fn every_message_is_declared_said_in_every_language_and_used() {
    let report = fundamental_messages_build::check(Path::new(env!("CARGO_MANIFEST_DIR")))
        .expect("the crate's messages are read");
    assert!(report.is_complete(), "{report}");
}

fn told(error: &ValidationError, language: &str) -> String {
    let catalog = Catalog::builder("en".parse().expect("a locale")).build();
    let locale: Locale = language.parse().expect("a locale");
    with(Arc::new(catalog.translator(&locale)), || {
        error.localized_message().into_owned()
    })
}

#[test]
fn each_validation_error_is_said_by_its_message() {
    let said = [
        (
            ValidationError::required("naam"),
            m::validate::required("naam"),
        ),
        (
            ValidationError::too_short("naam", 2, 1),
            m::validate::too_short("naam", 2, 1),
        ),
        (
            ValidationError::too_long("naam", 5, 9),
            m::validate::too_long("naam", 5, 9),
        ),
        (
            ValidationError::too_small("aantal", 1, 0),
            m::validate::too_small("aantal", 1, 0),
        ),
        (
            ValidationError::too_large("aantal", 9, 12),
            m::validate::too_large("aantal", 9, 12),
        ),
        (
            ValidationError::not_unique("naam"),
            m::validate::not_unique("naam"),
        ),
        (
            ValidationError::invalid_email("adres"),
            m::validate::invalid_email("adres"),
        ),
        (
            ValidationError::invalid_url("link"),
            m::validate::invalid_url("link"),
        ),
        (
            ValidationError::invalid_uuid("id"),
            m::validate::invalid_uuid("id"),
        ),
        (
            ValidationError::pattern_mismatch("code", "[0-9]+"),
            m::validate::pattern_mismatch("code"),
        ),
        (
            ValidationError::not_in_list("kleur", &["rood", "blauw"]),
            m::validate::not_in_list("kleur", "rood, blauw"),
        ),
    ];
    for (error, message) in said {
        for language in ["en", "nl"] {
            assert_eq!(
                told(&error, language),
                message.format_in(language),
                "{} in {language}",
                error.code
            );
        }
    }
    assert_eq!(
        told(&ValidationError::required("naam"), "nl"),
        "naam is verplicht"
    );
}
