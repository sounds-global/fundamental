//! An amount written for whoever reads the page.

use std::sync::Arc;

use fundamental_core::i18n::Locale;
use fundamental_core::money::{Denomination, Money};
use fundamental_i18n::{Catalog, MoneyFormatter, MoneyStyle};

fn locale(tag: &str) -> Locale {
    tag.parse().expect("a locale")
}

fn nine_and_a_half_thousand() -> Money {
    Money::from_cents(950_000, Denomination::Eur).expect("an amount")
}

#[test]
fn an_amount_is_written_in_the_readers_language() {
    let catalog = Catalog::builder(locale("en")).build();
    let written = fundamental_core::i18n::with(Arc::new(catalog.translator(&locale("nl"))), || {
        MoneyFormatter::for_reader()
            .format(nine_and_a_half_thousand(), MoneyStyle::WholeWhenIntegral)
    });
    assert_eq!(written, "€\u{a0}9.500");
}

#[test]
fn an_amount_with_no_reader_is_written_in_english() {
    assert_eq!(
        MoneyFormatter::for_reader()
            .format(nine_and_a_half_thousand(), MoneyStyle::WholeWhenIntegral),
        "€\u{a0}9,500"
    );
}
