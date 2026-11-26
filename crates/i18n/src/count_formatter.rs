//! [`CountFormatter`] — a whole number of things, grouped the way one reader's locale groups it.

use fixed_decimal::Decimal;
use fundamental_core::i18n::Locale;
use icu_decimal::DecimalFormatter;
use icu_locale_core::Locale as IcuLocale;
use tracing::warn;

/// Writes a count the way a reader in one locale expects to read it: `20.000` in Dutch.
pub struct CountFormatter {
    decimal: Option<DecimalFormatter>,
}

impl CountFormatter {
    /// A formatter for `locale`.
    ///
    /// Never fails: an uncovered locale falls back to root grouping, or bare digits.
    #[must_use]
    pub fn new(locale: &Locale) -> Self {
        let language = locale.language();
        let icu_locale = IcuLocale::try_from_str(language).unwrap_or_else(|error| {
            warn!(language = %language, %error, "locale not understood by ICU; using root grouping");
            IcuLocale::UNKNOWN
        });
        let decimal = DecimalFormatter::try_new((&icu_locale).into(), Default::default())
            .or_else(|error| {
                warn!(language = %language, %error, "no CLDR decimal data for locale; using root grouping");
                DecimalFormatter::try_new(Default::default(), Default::default())
            })
            .map_err(|error| warn!(%error, "no CLDR decimal data at all; writing counts as bare digits"))
            .ok();
        Self { decimal }
    }

    /// Writes `count`.
    #[must_use]
    pub fn format(&self, count: u64) -> String {
        match &self.decimal {
            Some(decimal) => decimal.format_to_string(&Decimal::from(count)),
            None => count.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formatter(language: &str) -> CountFormatter {
        CountFormatter::new(&language.parse().expect("locale"))
    }

    #[test]
    fn nl_groups_with_a_dot() {
        let nl = formatter("nl");
        assert_eq!(nl.format(3_500), "3.500");
        assert_eq!(nl.format(20_000_000), "20.000.000");
    }

    #[test]
    fn en_groups_with_a_comma() {
        let en = formatter("en");
        assert_eq!(en.format(3_500), "3,500");
        assert_eq!(en.format(20_000_000), "20,000,000");
    }

    #[test]
    fn a_count_below_a_thousand_is_ungrouped() {
        assert_eq!(formatter("nl").format(500), "500");
        assert_eq!(formatter("en").format(0), "0");
    }

    #[test]
    fn a_ten_figure_month_keeps_every_digit() {
        assert_eq!(formatter("en").format(15_000_000_000), "15,000,000,000");
    }

    #[test]
    fn one_count_differs_only_in_punctuation() {
        let digits = |text: String| {
            text.chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
        };
        assert_eq!(
            digits(formatter("nl").format(150_000)),
            digits(formatter("en").format(150_000))
        );
    }

    #[test]
    fn a_formatter_can_be_shared_across_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<CountFormatter>();
    }
}
