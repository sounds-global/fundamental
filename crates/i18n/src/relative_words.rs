//! [`RelativeWords`] — CLDR's relative-time words for the languages the framework carries.

use icu_plurals::PluralCategory;

use crate::RelativeUnit;

/// The relative-time words of one language: the words CLDR gives particular distances
///
/// Pinned to CLDR here because ICU4X's relative-time formatter is not semver-stable.
#[derive(Debug)]
pub(crate) struct RelativeWords {
    language: &'static str,
    units: [UnitWords; 6],
}

#[derive(Debug)]
struct UnitWords {
    named: &'static [(i8, &'static str)],
    future: Plural,
    past: Plural,
}

/// A pattern per plural category, `{0}` standing for the number.
#[derive(Debug)]
struct Plural {
    other: &'static str,
    special: &'static [(PluralCategory, &'static str)],
}

/// A language whose numbers fall in `one` or `other`, as both `nl` and `en` do.
macro_rules! plural {
    ($one:literal, $other:literal) => {
        Plural {
            other: $other,
            special: &[(PluralCategory::One, $one)],
        }
    };
}

const fn unit(named: &'static [(i8, &'static str)], future: Plural, past: Plural) -> UnitWords {
    UnitWords {
        named,
        future,
        past,
    }
}

const TABLE: [RelativeWords; 2] = [
    RelativeWords {
        language: "en",
        units: [
            unit(
                &[(0, "this minute")],
                plural!("in {0} minute", "in {0} minutes"),
                plural!("{0} minute ago", "{0} minutes ago"),
            ),
            unit(
                &[(0, "this hour")],
                plural!("in {0} hour", "in {0} hours"),
                plural!("{0} hour ago", "{0} hours ago"),
            ),
            unit(
                &[(-1, "yesterday"), (0, "today"), (1, "tomorrow")],
                plural!("in {0} day", "in {0} days"),
                plural!("{0} day ago", "{0} days ago"),
            ),
            unit(
                &[(-1, "last week"), (0, "this week"), (1, "next week")],
                plural!("in {0} week", "in {0} weeks"),
                plural!("{0} week ago", "{0} weeks ago"),
            ),
            unit(
                &[(-1, "last month"), (0, "this month"), (1, "next month")],
                plural!("in {0} month", "in {0} months"),
                plural!("{0} month ago", "{0} months ago"),
            ),
            unit(
                &[(-1, "last year"), (0, "this year"), (1, "next year")],
                plural!("in {0} year", "in {0} years"),
                plural!("{0} year ago", "{0} years ago"),
            ),
        ],
    },
    RelativeWords {
        language: "nl",
        units: [
            unit(
                &[(0, "binnen een minuut")],
                plural!("over {0} minuut", "over {0} minuten"),
                plural!("{0} minuut geleden", "{0} minuten geleden"),
            ),
            unit(
                &[(0, "binnen een uur")],
                plural!("over {0} uur", "over {0} uur"),
                plural!("{0} uur geleden", "{0} uur geleden"),
            ),
            unit(
                &[
                    (-2, "eergisteren"),
                    (-1, "gisteren"),
                    (0, "vandaag"),
                    (1, "morgen"),
                    (2, "overmorgen"),
                ],
                plural!("over {0} dag", "over {0} dagen"),
                plural!("{0} dag geleden", "{0} dagen geleden"),
            ),
            unit(
                &[(-1, "vorige week"), (0, "deze week"), (1, "volgende week")],
                plural!("over {0} week", "over {0} weken"),
                plural!("{0} week geleden", "{0} weken geleden"),
            ),
            unit(
                &[
                    (-1, "vorige maand"),
                    (0, "deze maand"),
                    (1, "volgende maand"),
                ],
                plural!("over {0} maand", "over {0} maanden"),
                plural!("{0} maand geleden", "{0} maanden geleden"),
            ),
            unit(
                &[(-1, "vorig jaar"), (0, "dit jaar"), (1, "volgend jaar")],
                plural!("over {0} jaar", "over {0} jaar"),
                plural!("{0} jaar geleden", "{0} jaar geleden"),
            ),
        ],
    },
];

impl RelativeWords {
    /// The words for `language` (a bare language subtag), if the framework carries them.
    pub(crate) fn for_language(language: &str) -> Option<&'static Self> {
        TABLE.iter().find(|words| words.language == language)
    }

    /// The word CLDR gives `amount` units from now, if it names that distance.
    pub(crate) fn named(&self, unit: RelativeUnit, amount: i64) -> Option<&'static str> {
        self.units[unit.index()]
            .named
            .iter()
            .find(|(distance, _)| i64::from(*distance) == amount)
            .map(|(_, word)| *word)
    }

    /// The pattern for a distance in the future or the past whose number falls in `category`.
    pub(crate) fn pattern(
        &self,
        unit: RelativeUnit,
        future: bool,
        category: PluralCategory,
    ) -> &'static str {
        let words = &self.units[unit.index()];
        let plural = if future { &words.future } else { &words.past };
        plural
            .special
            .iter()
            .find(|(special, _)| *special == category)
            .map_or(plural.other, |(_, pattern)| *pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLDR release [`TABLE`] is transcribed from.
    const CLDR_VERSION: &str = "48.2.1";

    /// The `icu_datetime_data` release carrying CLDR [`CLDR_VERSION`]: the lockfile test holds the pin.
    const ICU_DATA_VERSION: &str = "2.3.0";

    /// CLDR 48.2.1's `dateFields` (long forms), from `cldr-json/cldr-dates-full/main/{en,nl}`.
    const CLDR_NAMED: &[(&str, RelativeUnit, i64, &str)] = &[
        ("en", RelativeUnit::Minute, 0, "this minute"),
        ("en", RelativeUnit::Hour, 0, "this hour"),
        ("en", RelativeUnit::Day, -1, "yesterday"),
        ("en", RelativeUnit::Day, 0, "today"),
        ("en", RelativeUnit::Day, 1, "tomorrow"),
        ("en", RelativeUnit::Week, -1, "last week"),
        ("en", RelativeUnit::Week, 0, "this week"),
        ("en", RelativeUnit::Week, 1, "next week"),
        ("en", RelativeUnit::Month, -1, "last month"),
        ("en", RelativeUnit::Month, 0, "this month"),
        ("en", RelativeUnit::Month, 1, "next month"),
        ("en", RelativeUnit::Year, -1, "last year"),
        ("en", RelativeUnit::Year, 0, "this year"),
        ("en", RelativeUnit::Year, 1, "next year"),
        ("nl", RelativeUnit::Minute, 0, "binnen een minuut"),
        ("nl", RelativeUnit::Hour, 0, "binnen een uur"),
        ("nl", RelativeUnit::Day, -2, "eergisteren"),
        ("nl", RelativeUnit::Day, -1, "gisteren"),
        ("nl", RelativeUnit::Day, 0, "vandaag"),
        ("nl", RelativeUnit::Day, 1, "morgen"),
        ("nl", RelativeUnit::Day, 2, "overmorgen"),
        ("nl", RelativeUnit::Week, -1, "vorige week"),
        ("nl", RelativeUnit::Week, 0, "deze week"),
        ("nl", RelativeUnit::Week, 1, "volgende week"),
        ("nl", RelativeUnit::Month, -1, "vorige maand"),
        ("nl", RelativeUnit::Month, 0, "deze maand"),
        ("nl", RelativeUnit::Month, 1, "volgende maand"),
        ("nl", RelativeUnit::Year, -1, "vorig jaar"),
        ("nl", RelativeUnit::Year, 0, "dit jaar"),
        ("nl", RelativeUnit::Year, 1, "volgend jaar"),
    ];

    const CLDR_PATTERNS: &[(&str, RelativeUnit, bool, PluralCategory, &str)] = &[
        (
            "en",
            RelativeUnit::Minute,
            true,
            PluralCategory::One,
            "in {0} minute",
        ),
        (
            "en",
            RelativeUnit::Minute,
            true,
            PluralCategory::Other,
            "in {0} minutes",
        ),
        (
            "en",
            RelativeUnit::Minute,
            false,
            PluralCategory::One,
            "{0} minute ago",
        ),
        (
            "en",
            RelativeUnit::Minute,
            false,
            PluralCategory::Other,
            "{0} minutes ago",
        ),
        (
            "en",
            RelativeUnit::Hour,
            true,
            PluralCategory::One,
            "in {0} hour",
        ),
        (
            "en",
            RelativeUnit::Hour,
            true,
            PluralCategory::Other,
            "in {0} hours",
        ),
        (
            "en",
            RelativeUnit::Hour,
            false,
            PluralCategory::One,
            "{0} hour ago",
        ),
        (
            "en",
            RelativeUnit::Hour,
            false,
            PluralCategory::Other,
            "{0} hours ago",
        ),
        (
            "en",
            RelativeUnit::Day,
            true,
            PluralCategory::One,
            "in {0} day",
        ),
        (
            "en",
            RelativeUnit::Day,
            true,
            PluralCategory::Other,
            "in {0} days",
        ),
        (
            "en",
            RelativeUnit::Day,
            false,
            PluralCategory::One,
            "{0} day ago",
        ),
        (
            "en",
            RelativeUnit::Day,
            false,
            PluralCategory::Other,
            "{0} days ago",
        ),
        (
            "en",
            RelativeUnit::Week,
            true,
            PluralCategory::One,
            "in {0} week",
        ),
        (
            "en",
            RelativeUnit::Week,
            true,
            PluralCategory::Other,
            "in {0} weeks",
        ),
        (
            "en",
            RelativeUnit::Week,
            false,
            PluralCategory::One,
            "{0} week ago",
        ),
        (
            "en",
            RelativeUnit::Week,
            false,
            PluralCategory::Other,
            "{0} weeks ago",
        ),
        (
            "en",
            RelativeUnit::Month,
            true,
            PluralCategory::One,
            "in {0} month",
        ),
        (
            "en",
            RelativeUnit::Month,
            true,
            PluralCategory::Other,
            "in {0} months",
        ),
        (
            "en",
            RelativeUnit::Month,
            false,
            PluralCategory::One,
            "{0} month ago",
        ),
        (
            "en",
            RelativeUnit::Month,
            false,
            PluralCategory::Other,
            "{0} months ago",
        ),
        (
            "en",
            RelativeUnit::Year,
            true,
            PluralCategory::One,
            "in {0} year",
        ),
        (
            "en",
            RelativeUnit::Year,
            true,
            PluralCategory::Other,
            "in {0} years",
        ),
        (
            "en",
            RelativeUnit::Year,
            false,
            PluralCategory::One,
            "{0} year ago",
        ),
        (
            "en",
            RelativeUnit::Year,
            false,
            PluralCategory::Other,
            "{0} years ago",
        ),
        (
            "nl",
            RelativeUnit::Minute,
            true,
            PluralCategory::One,
            "over {0} minuut",
        ),
        (
            "nl",
            RelativeUnit::Minute,
            true,
            PluralCategory::Other,
            "over {0} minuten",
        ),
        (
            "nl",
            RelativeUnit::Minute,
            false,
            PluralCategory::One,
            "{0} minuut geleden",
        ),
        (
            "nl",
            RelativeUnit::Minute,
            false,
            PluralCategory::Other,
            "{0} minuten geleden",
        ),
        (
            "nl",
            RelativeUnit::Hour,
            true,
            PluralCategory::One,
            "over {0} uur",
        ),
        (
            "nl",
            RelativeUnit::Hour,
            true,
            PluralCategory::Other,
            "over {0} uur",
        ),
        (
            "nl",
            RelativeUnit::Hour,
            false,
            PluralCategory::One,
            "{0} uur geleden",
        ),
        (
            "nl",
            RelativeUnit::Hour,
            false,
            PluralCategory::Other,
            "{0} uur geleden",
        ),
        (
            "nl",
            RelativeUnit::Day,
            true,
            PluralCategory::One,
            "over {0} dag",
        ),
        (
            "nl",
            RelativeUnit::Day,
            true,
            PluralCategory::Other,
            "over {0} dagen",
        ),
        (
            "nl",
            RelativeUnit::Day,
            false,
            PluralCategory::One,
            "{0} dag geleden",
        ),
        (
            "nl",
            RelativeUnit::Day,
            false,
            PluralCategory::Other,
            "{0} dagen geleden",
        ),
        (
            "nl",
            RelativeUnit::Week,
            true,
            PluralCategory::One,
            "over {0} week",
        ),
        (
            "nl",
            RelativeUnit::Week,
            true,
            PluralCategory::Other,
            "over {0} weken",
        ),
        (
            "nl",
            RelativeUnit::Week,
            false,
            PluralCategory::One,
            "{0} week geleden",
        ),
        (
            "nl",
            RelativeUnit::Week,
            false,
            PluralCategory::Other,
            "{0} weken geleden",
        ),
        (
            "nl",
            RelativeUnit::Month,
            true,
            PluralCategory::One,
            "over {0} maand",
        ),
        (
            "nl",
            RelativeUnit::Month,
            true,
            PluralCategory::Other,
            "over {0} maanden",
        ),
        (
            "nl",
            RelativeUnit::Month,
            false,
            PluralCategory::One,
            "{0} maand geleden",
        ),
        (
            "nl",
            RelativeUnit::Month,
            false,
            PluralCategory::Other,
            "{0} maanden geleden",
        ),
        (
            "nl",
            RelativeUnit::Year,
            true,
            PluralCategory::One,
            "over {0} jaar",
        ),
        (
            "nl",
            RelativeUnit::Year,
            true,
            PluralCategory::Other,
            "over {0} jaar",
        ),
        (
            "nl",
            RelativeUnit::Year,
            false,
            PluralCategory::One,
            "{0} jaar geleden",
        ),
        (
            "nl",
            RelativeUnit::Year,
            false,
            PluralCategory::Other,
            "{0} jaar geleden",
        ),
    ];

    #[test]
    fn every_pattern_is_cldrs() {
        for (language, unit, future, category, expected) in CLDR_PATTERNS {
            let words = RelativeWords::for_language(language).expect("carried");
            assert_eq!(
                words.pattern(*unit, *future, *category),
                *expected,
                "{language} {unit:?} future={future} {category:?}"
            );
        }
    }

    #[test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn every_named_distance_is_cldrs_and_no_other_is_named() {
        for language in ["en", "nl"] {
            let words = RelativeWords::for_language(language).expect("carried");
            for unit in RelativeUnit::ALL {
                for amount in -3..=3 {
                    let expected = CLDR_NAMED
                        .iter()
                        .find(|(l, u, a, _)| *l == language && *u == unit && *a == amount)
                        .map(|(_, _, _, word)| *word);
                    assert_eq!(
                        words.named(unit, amount),
                        expected,
                        "{language} {unit:?} {amount}"
                    );
                }
            }
        }
    }

    #[test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn every_pattern_has_one_placeholder() {
        for words in &TABLE {
            for unit in RelativeUnit::ALL {
                for future in [true, false] {
                    for category in [PluralCategory::One, PluralCategory::Other] {
                        let pattern = words.pattern(unit, future, category);
                        assert_eq!(
                            pattern.matches("{0}").count(),
                            1,
                            "{} {unit:?} {pattern}",
                            words.language
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_category_without_its_own_pattern_uses_other() {
        let nl = RelativeWords::for_language("nl").expect("nl is carried");
        assert_eq!(
            nl.pattern(RelativeUnit::Day, true, PluralCategory::Few),
            "over {0} dagen"
        );
    }

    #[test]
    fn a_language_without_a_row_is_not_carried() {
        assert!(RelativeWords::for_language("de").is_none());
        assert!(RelativeWords::for_language("").is_none());
    }

    #[test]
    fn the_table_is_read_against_the_icu_data_the_lockfile_pins() {
        let lockfile = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"));
        let pinned = lockfile
            .split("[[package]]")
            .find(|package| package.contains("name = \"icu_datetime_data\""))
            .and_then(|package| {
                package
                    .lines()
                    .find_map(|line| line.strip_prefix("version = \""))
                    .map(|version| version.trim_end_matches('"'))
            });
        assert_eq!(
            pinned,
            Some(ICU_DATA_VERSION),
            "icu_datetime_data moved: re-read the relative words against its CLDR release and update CLDR_VERSION ({CLDR_VERSION})"
        );
    }
}
