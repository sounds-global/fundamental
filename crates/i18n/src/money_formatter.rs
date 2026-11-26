//! [`MoneyFormatter`] — an exact amount of money, written for one reader's locale.

use fixed_decimal::Decimal;
use fundamental_core::i18n::Locale;
use fundamental_core::money::{Denomination, Money};
use icu_decimal::DecimalFormatter;
use icu_locale_core::Locale as IcuLocale;
use tracing::warn;

use crate::MoneyStyle;

/// Writes a [`Money`] amount the way a reader in one locale expects to read it.
///
/// Symbol placement is pinned to CLDR 47 because ICU4X's currency formatter is not yet semver-stable.
pub struct MoneyFormatter {
    decimal: Option<DecimalFormatter>,
    language: String,
}

/// Where a locale puts the symbol and the sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    /// `¤#,##0.00` — `$2,500.00`, `-$1.70` (en).
    PrefixTight,
    /// `¤ #,##0.00;¤ -#,##0.00` with a no-break space — `€ 2.500,00`, `€ -1,70` (nl, en-NL).
    PrefixSpacedSignInside,
}

impl MoneyFormatter {
    /// A formatter for `locale`.
    ///
    /// Never fails: an uncovered locale or placement renders the ISO form.
    #[must_use]
    pub fn new(locale: &Locale) -> Self {
        let language = locale.language().to_owned();
        let icu_locale = IcuLocale::try_from_str(&language).unwrap_or_else(|error| {
            warn!(language = %language, %error, "locale not understood by ICU; using root separators");
            IcuLocale::UNKNOWN
        });
        let decimal = DecimalFormatter::try_new((&icu_locale).into(), Default::default())
            .or_else(|error| {
                warn!(language = %language, %error, "no CLDR decimal data for locale; using root separators");
                DecimalFormatter::try_new(Default::default(), Default::default())
            });
        let decimal = decimal
            .map_err(|error| {
                warn!(%error, "no CLDR decimal data at all; rendering money in ISO form");
            })
            .ok();
        Self { decimal, language }
    }

    /// A formatter for the current translator's language, English where none is known.
    #[must_use]
    pub fn for_reader() -> Self {
        let language = fundamental_core::i18n::current()
            .and_then(|reader| reader.locale_tag().parse::<Locale>().ok())
            .unwrap_or_else(|| Locale::new("en", None::<&str>));
        Self::new(&language)
    }

    /// Writes `amount` in `style`.
    #[must_use]
    pub fn format(&self, amount: Money, style: MoneyStyle) -> String {
        let denomination = amount.denomination();
        let (Some(decimal), Some((symbol, placement))) =
            (self.decimal.as_ref(), written(&self.language, denomination))
        else {
            warn!(language = %self.language, denomination = denomination.code(), "no pinned currency placement; rendering money in ISO form");
            return iso_form(amount, style);
        };
        let digits = decimal.format_to_string(&magnitude(amount, style));
        let sign = if amount.micros() < 0 { "-" } else { "" };
        match placement {
            Placement::PrefixTight => format!("{sign}{symbol}{digits}"),
            Placement::PrefixSpacedSignInside => format!("{symbol}\u{a0}{sign}{digits}"),
        }
    }
}

/// The symbol a locale writes a denomination with and where it puts it, from CLDR 47.
fn written(language: &str, denomination: Denomination) -> Option<(&'static str, Placement)> {
    match (language, denomination) {
        ("en" | "nl", Denomination::Eur) => Some(("€", Placement::PrefixSpacedSignInside)),
        ("en", Denomination::Usd) => Some(("$", Placement::PrefixTight)),
        ("nl", Denomination::Usd) => Some(("US$", Placement::PrefixSpacedSignInside)),
        _ => None,
    }
}

/// The absolute amount as an exact decimal, padded or trimmed for `style`.
fn magnitude(amount: Money, style: MoneyStyle) -> Decimal {
    let mut decimal = Decimal::from(amount.micros().unsigned_abs());
    decimal.multiply_pow10(-6);
    decimal.trim_end();
    // Minor digits are small per denomination; showing every micro digit is still exact.
    let minor = i16::try_from(amount.denomination().minor_digits()).unwrap_or(6);
    let whole = amount.micros() % Money::MICROS_PER_MAJOR == 0;
    if !(style == MoneyStyle::WholeWhenIntegral && whole) {
        decimal.pad_end(-minor);
    }
    decimal
}

/// `EUR 2500.00`: readable in any language.
fn iso_form(amount: Money, style: MoneyStyle) -> String {
    let sign = if amount.micros() < 0 { "-" } else { "" };
    format!(
        "{} {sign}{}",
        amount.denomination().code(),
        magnitude(amount, style)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nl() -> MoneyFormatter {
        MoneyFormatter::new(&"nl".parse().expect("locale"))
    }

    fn en() -> MoneyFormatter {
        MoneyFormatter::new(&"en".parse().expect("locale"))
    }

    fn eur(micros: i64) -> Money {
        Money::from_micros(micros, Denomination::Eur)
    }

    #[test]
    fn nl_renders_euro_with_a_space_and_dot_grouping() {
        assert_eq!(
            nl().format(eur(2_500_000_000), MoneyStyle::WholeWhenIntegral),
            "€\u{a0}2.500"
        );
        assert_eq!(
            nl().format(eur(2_500_000_000), MoneyStyle::Exact),
            "€\u{a0}2.500,00"
        );
    }

    #[test]
    fn en_renders_euro_with_a_space_and_comma_grouping() {
        assert_eq!(
            en().format(eur(2_500_000_000), MoneyStyle::WholeWhenIntegral),
            "€\u{a0}2,500"
        );
        assert_eq!(
            en().format(eur(99_000_000), MoneyStyle::Exact),
            "€\u{a0}99.00"
        );
    }

    #[test]
    fn a_sub_cent_rate_keeps_every_digit() {
        assert_eq!(
            nl().format(eur(2_500), MoneyStyle::WholeWhenIntegral),
            "€\u{a0}0,0025"
        );
        assert_eq!(en().format(eur(40_000), MoneyStyle::Exact), "€\u{a0}0.04");
        assert_eq!(en().format(eur(37), MoneyStyle::Exact), "€\u{a0}0.000037");
    }

    #[test]
    fn a_negative_amount_keeps_its_sign_where_cldr_puts_it() {
        assert_eq!(
            en().format(eur(-1_700_000), MoneyStyle::Exact),
            "€\u{a0}-1.70"
        );
        assert_eq!(
            nl().format(eur(-1_700_000), MoneyStyle::Exact),
            "€\u{a0}-1,70"
        );
    }

    #[test]
    fn the_dollar_is_written_as_each_locale_writes_it() {
        let usd = Money::from_micros(266_420_000, Denomination::Usd);
        assert_eq!(en().format(usd, MoneyStyle::Exact), "$266.42");
        assert_eq!(nl().format(usd, MoneyStyle::Exact), "US$\u{a0}266,42");
    }

    #[test]
    fn an_unlisted_locale_falls_back_to_the_iso_form() {
        let de = MoneyFormatter::new(&"de".parse().expect("locale"));
        assert_eq!(
            de.format(eur(2_500_000_000), MoneyStyle::Exact),
            "EUR 2500.00"
        );
    }

    #[test]
    fn one_figure_differs_only_in_punctuation() {
        let figure = eur(123_456_789_000);
        let digits = |text: String| {
            text.chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
        };
        assert_eq!(
            digits(nl().format(figure, MoneyStyle::Exact)),
            digits(en().format(figure, MoneyStyle::Exact))
        );
    }

    #[test]
    fn a_formatter_can_be_shared_across_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<MoneyFormatter>();
    }
}
