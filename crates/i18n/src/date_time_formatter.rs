//! [`DateTimeFormatter`] — dates, times and relative phrases for one reader's locale and zone.

use std::array;
use std::sync::OnceLock;

use chrono::{
    DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Offset, SecondsFormat, TimeDelta,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use fundamental_core::i18n::Locale;
use icu_calendar::{Date, Gregorian};
use icu_datetime::FixedCalendarDateTimeFormatter;
use icu_datetime::fieldsets::builder::{DateFields, FieldSetBuilder, ZoneStyle};
use icu_datetime::fieldsets::enums::CompositeFieldSet;
use icu_datetime::options::{Length, TimePrecision};
use icu_locale_core::Locale as IcuLocale;
use icu_plurals::{PluralCategory, PluralRules};
use icu_time::zone::{TimeZoneInfo, UtcOffset, models::AtTime};
use icu_time::{DateTime as IcuDateTime, Time, TimeZone as IcuTimeZone, ZonedDateTime};
use tracing::warn;

use crate::relative_words::RelativeWords;
use crate::{CountFormatter, DateStyle, DayStyle, RelativeUnit, TimeStyle};

type Pattern = FixedCalendarDateTimeFormatter<Gregorian, CompositeFieldSet>;
type Input = ZonedDateTime<Gregorian, TimeZoneInfo<AtTime>>;

/// One slot per shape: four dates, two days, four times, and every date-time pair.
const SLOTS: usize = 4 + 2 + 4 + 4 * 4;

/// Writes dates, times and relative phrases for one locale, on one IANA time zone's wall clock.
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use fundamental_i18n::{DateStyle, DateTimeFormatter, DayStyle, RelativeUnit, TimeStyle, Tz};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let amsterdam: Tz = "Europe/Amsterdam".parse()?;
/// let nl = DateTimeFormatter::new(&"nl-NL".parse()?, amsterdam).ok_or("nl is carried")?;
/// let start = Utc.with_ymd_and_hms(2026, 10, 8, 12, 5, 0).single().ok_or("an instant")?;
///
/// assert_eq!(nl.day(start, DayStyle::Medium), "do 8 okt");
/// assert_eq!(nl.date(start, DateStyle::Full), "donderdag 8 oktober 2026");
/// assert_eq!(nl.time(start, TimeStyle::Long), "14:05:00 CEST");
/// assert_eq!(nl.relative(2, RelativeUnit::Day), "overmorgen");
/// # Ok(())
/// # }
/// ```
pub struct DateTimeFormatter {
    locale: Locale,
    zone: Tz,
    icu_zone: IcuTimeZone,
    vocabulary: Option<Vocabulary>,
}

struct Vocabulary {
    icu_locale: IcuLocale,
    words: &'static RelativeWords,
    plurals: Option<PluralRules>,
    count: CountFormatter,
    patterns: [OnceLock<Option<Pattern>>; SLOTS],
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Date(DateStyle),
    Day(DayStyle),
    Time(TimeStyle),
    DateTime(DateStyle, TimeStyle),
}

impl DateTimeFormatter {
    /// A formatter for `locale` on `zone`'s wall clock, or `None` for an uncarried language.
    #[must_use]
    pub fn new(locale: &Locale, zone: Tz) -> Option<Self> {
        let words = RelativeWords::for_language(locale.language())?;
        let icu_locale = IcuLocale::try_from_str(&locale.to_string())
            .or_else(|error| {
                warn!(locale = %locale, %error, "locale not understood by ICU; using its language alone");
                IcuLocale::try_from_str(locale.language())
            })
            .map_err(|error| warn!(locale = %locale, %error, "language not understood by ICU; no date vocabulary"))
            .ok()?;
        let plurals = PluralRules::try_new_cardinal((&icu_locale).into())
            .map_err(|error| warn!(locale = %locale, %error, "no CLDR plural rules; relative phrases use the `other` form"))
            .ok();
        Some(Self {
            locale: locale.clone(),
            zone,
            icu_zone: IcuTimeZone::from_iana_id(zone.name()),
            vocabulary: Some(Vocabulary {
                icu_locale,
                words,
                plurals,
                count: CountFormatter::new(locale),
                patterns: array::from_fn(|_| OnceLock::new()),
            }),
        })
    }

    /// A formatter that writes everything in ISO 8601.
    pub(crate) fn iso(locale: &Locale, zone: Tz) -> Self {
        Self {
            locale: locale.clone(),
            zone,
            icu_zone: IcuTimeZone::from_iana_id(zone.name()),
            vocabulary: None,
        }
    }

    /// The locale this formatter writes in.
    #[must_use]
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// The zone whose wall clock this formatter reads.
    #[must_use]
    pub fn zone(&self) -> Tz {
        self.zone
    }

    /// Whether this formatter writes in the locale's words, rather than in ISO 8601.
    #[must_use]
    pub fn is_localised(&self) -> bool {
        self.vocabulary.is_some()
    }

    /// The date `at` falls on in the zone: `donderdag 8 oktober 2026`, `8 Oct 2026`.
    #[must_use]
    pub fn date(&self, at: DateTime<Utc>, style: DateStyle) -> String {
        self.instant(at, Shape::Date(style))
    }

    /// The day `at` falls on in the zone, with its weekday and without its year: `do 8 okt`.
    #[must_use]
    pub fn day(&self, at: DateTime<Utc>, style: DayStyle) -> String {
        self.instant(at, Shape::Day(style))
    }

    /// The wall-clock time `at` shows in the zone: `14:05`, `14:05:00 CEST`.
    #[must_use]
    pub fn time(&self, at: DateTime<Utc>, style: TimeStyle) -> String {
        self.instant(at, Shape::Time(style))
    }

    /// The date and time `at` shows in the zone, joined as the locale joins them.
    #[must_use]
    pub fn date_time(&self, at: DateTime<Utc>, date: DateStyle, time: TimeStyle) -> String {
        self.instant(at, Shape::DateTime(date, time))
    }

    /// A calendar date that names no instant — a start date, a due day: `8 oktober 2026`.
    #[must_use]
    pub fn calendar_date(&self, date: NaiveDate, style: DateStyle) -> String {
        self.calendar(date, Shape::Date(style))
    }

    /// A calendar date that names no instant, with its weekday and without its year.
    #[must_use]
    pub fn calendar_day(&self, date: NaiveDate, style: DayStyle) -> String {
        self.calendar(date, Shape::Day(style))
    }

    /// `amount` units from now, in the locale's words: `morgen`, `over 3 dagen`,
    /// `2 weeks ago`. A negative amount is in the past.
    #[must_use]
    pub fn relative(&self, amount: i64, unit: RelativeUnit) -> String {
        let Some(vocabulary) = &self.vocabulary else {
            return iso_duration(amount, unit);
        };
        if let Some(word) = vocabulary.words.named(unit, amount) {
            return word.to_owned();
        }
        let magnitude = amount.unsigned_abs();
        let category = vocabulary
            .plurals
            .as_ref()
            .map_or(PluralCategory::Other, |rules| rules.category_for(magnitude));
        let number = vocabulary.count.format(magnitude);
        vocabulary
            .words
            .pattern(unit, amount >= 0, category)
            .replace("{0}", &number)
    }

    /// How many calendar days `at` lies from `now` in the zone, in the locale's words:
    /// `vandaag`, `morgen`, `over 3 dagen`.
    ///
    /// Counted in wall-clock dates, so a daylight-saving day is one day.
    #[must_use]
    pub fn relative_days(&self, at: DateTime<Utc>, now: DateTime<Utc>) -> String {
        self.relative(self.days_between(at, now), RelativeUnit::Day)
    }

    /// How many calendar days `at` lies from `now` on the zone's wall clock: `-1` for yesterday.
    #[must_use]
    pub fn days_between(&self, at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
        (self.local_date(at) - self.local_date(now)).num_days()
    }

    fn local_date(&self, at: DateTime<Utc>) -> NaiveDate {
        match self.local(at) {
            Some((local, _)) => local.date(),
            None => {
                warn!(zone = self.zone.name(), %at, "instant has no wall-clock time in its zone; counting days in UTC");
                at.date_naive()
            }
        }
    }

    /// The wall-clock time `at` shows in the zone and its UTC offset in seconds, if representable.
    fn local(&self, at: DateTime<Utc>) -> Option<(NaiveDateTime, i32)> {
        let offset = self
            .zone
            .offset_from_utc_datetime(&at.naive_utc())
            .fix()
            .local_minus_utc();
        at.naive_utc()
            .checked_add_signed(TimeDelta::seconds(i64::from(offset)))
            .map(|local| (local, offset))
    }

    fn instant(&self, at: DateTime<Utc>, shape: Shape) -> String {
        // A leap second shifted by the offset would read one second early; only UTC names it.
        if at.nanosecond() >= 1_000_000_000 {
            warn!(zone = self.zone.name(), %at, ?shape, "a leap second has no wall-clock time in its zone; writing it in UTC");
            return at.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        }
        let Some((local, offset)) = self.local(at) else {
            warn!(zone = self.zone.name(), %at, ?shape, "instant has no wall-clock time in its zone; writing it in UTC");
            return at.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        };
        self.write(local, offset, shape)
    }

    fn calendar(&self, date: NaiveDate, shape: Shape) -> String {
        let midnight = date.and_time(NaiveTime::MIN);
        let offset = self
            .zone
            .offset_from_utc_datetime(&midnight)
            .fix()
            .local_minus_utc();
        self.write(midnight, offset, shape)
    }

    fn write(&self, local: NaiveDateTime, offset: i32, shape: Shape) -> String {
        let Some(vocabulary) = &self.vocabulary else {
            return iso_form(local, offset, shape);
        };
        let Some(pattern) = vocabulary.pattern(shape) else {
            return iso_form(local, offset, shape);
        };
        match self.input(local, offset) {
            Some(input) => pattern.format(&input).to_string(),
            None => {
                warn!(locale = %self.locale, %local, ?shape, "value outside what ICU represents; writing it in ISO 8601");
                iso_form(local, offset, shape)
            }
        }
    }

    fn input(&self, local: NaiveDateTime, offset: i32) -> Option<Input> {
        let date = Date::try_new_gregorian(
            local.year(),
            u8::try_from(local.month()).ok()?,
            u8::try_from(local.day()).ok()?,
        )
        .ok()?;
        let time = Time::try_new(
            u8::try_from(local.hour()).ok()?,
            u8::try_from(local.minute()).ok()?,
            u8::try_from(local.second()).ok()?,
            local.nanosecond(),
        )
        .ok()?;
        let zone = self
            .icu_zone
            .with_offset(UtcOffset::try_from_seconds(offset).ok())
            .at_date_time(IcuDateTime { date, time });
        Some(ZonedDateTime { date, time, zone })
    }
}

impl Vocabulary {
    fn pattern(&self, shape: Shape) -> Option<&Pattern> {
        self.patterns[shape.slot()]
            .get_or_init(|| {
                let fields = shape.fields();
                let built = fields
                    .build_composite()
                    .map_err(|error| format!("{error:?}"))
                    .and_then(|fields| {
                        Pattern::try_new((&self.icu_locale).into(), fields)
                            .map_err(|error| format!("{error:?}"))
                    });
                built
                    .map_err(|error| {
                        warn!(locale = %self.icu_locale, ?shape, %error, "no CLDR pattern for this style; writing it in ISO 8601");
                    })
                    .ok()
            })
            .as_ref()
    }
}

impl Shape {
    fn slot(self) -> usize {
        let date = |style: DateStyle| match style {
            DateStyle::Full => 0,
            DateStyle::Long => 1,
            DateStyle::Medium => 2,
            DateStyle::Short => 3,
        };
        let time = |style: TimeStyle| match style {
            TimeStyle::Full => 0,
            TimeStyle::Long => 1,
            TimeStyle::Medium => 2,
            TimeStyle::Short => 3,
        };
        match self {
            Self::Date(style) => date(style),
            Self::Day(DayStyle::Long) => 4,
            Self::Day(DayStyle::Medium) => 5,
            Self::Time(style) => 6 + time(style),
            Self::DateTime(date_style, time_style) => 10 + 4 * date(date_style) + time(time_style),
        }
    }

    fn fields(self) -> FieldSetBuilder {
        let mut fields = FieldSetBuilder::new();
        let (date, time) = match self {
            Self::Date(date) => (Some(date), None),
            Self::DateTime(date, time) => (Some(date), Some(time)),
            Self::Time(time) => (None, Some(time)),
            Self::Day(day) => {
                fields.date_fields = Some(DateFields::MDE);
                fields.length = Some(match day {
                    DayStyle::Long => Length::Long,
                    DayStyle::Medium => Length::Medium,
                });
                (None, None)
            }
        };
        if let Some(date) = date {
            let (date_fields, length) = match date {
                DateStyle::Full => (DateFields::YMDE, Length::Long),
                DateStyle::Long => (DateFields::YMD, Length::Long),
                DateStyle::Medium => (DateFields::YMD, Length::Medium),
                DateStyle::Short => (DateFields::YMD, Length::Short),
            };
            fields.date_fields = Some(date_fields);
            fields.length = Some(length);
        }
        if let Some(time) = time {
            fields.time_precision = Some(match time {
                TimeStyle::Short => TimePrecision::Minute,
                TimeStyle::Full | TimeStyle::Long | TimeStyle::Medium => TimePrecision::Second,
            });
            fields.zone_style = match time {
                TimeStyle::Full => Some(ZoneStyle::SpecificLong),
                TimeStyle::Long => Some(ZoneStyle::SpecificShort),
                TimeStyle::Medium | TimeStyle::Short => None,
            };
            if fields.length.is_none() {
                fields.length = Some(Length::Medium);
            }
        }
        fields
    }
}

/// `shape` in ISO 8601: `2026-10-08`, `14:05:00+02:00`.
fn iso_form(local: NaiveDateTime, offset: i32, shape: Shape) -> String {
    let time = |style: TimeStyle| match style {
        TimeStyle::Full | TimeStyle::Long => {
            format!("{}{}", local.format("%H:%M:%S"), iso_offset(offset))
        }
        TimeStyle::Medium => local.format("%H:%M:%S").to_string(),
        TimeStyle::Short => local.format("%H:%M").to_string(),
    };
    match shape {
        Shape::Date(_) => local.format("%Y-%m-%d").to_string(),
        Shape::Day(_) => local.format("--%m-%d").to_string(),
        Shape::Time(style) => time(style),
        Shape::DateTime(_, style) => format!("{}T{}", local.format("%Y-%m-%d"), time(style)),
    }
}

/// `+02:00`, `-03:30`, `Z`.
fn iso_offset(seconds: i32) -> String {
    if seconds == 0 {
        return "Z".to_owned();
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `amount` units as an ISO 8601 duration: `P3D`, `-P2W`, `PT5M`.
fn iso_duration(amount: i64, unit: RelativeUnit) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let (designator, in_time) = unit.iso_designator();
    let time = if in_time { "T" } else { "" };
    format!("{sign}P{time}{}{designator}", amount.unsigned_abs())
}

#[cfg(test)]
mod tests {
    use chrono_tz::Europe::Amsterdam;
    use proptest::prelude::*;

    use super::*;

    fn formatter(tag: &str) -> DateTimeFormatter {
        DateTimeFormatter::new(&tag.parse().expect("locale"), Amsterdam).expect("carried")
    }

    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
            .single()
            .expect("valid instant")
    }

    /// Thursday 8 October 2026, 14:05:00 in Amsterdam (CEST, UTC+2).
    fn thursday() -> DateTime<Utc> {
        utc(2026, 10, 8, 12, 5, 0)
    }

    // Expected strings are CLDR 48.2.1's, for `nl` and `en-GB`.

    #[test]
    fn nl_nl_writes_every_date_style_as_cldr_does() {
        let nl = formatter("nl-NL");
        assert_eq!(
            nl.date(thursday(), DateStyle::Full),
            "donderdag 8 oktober 2026"
        );
        assert_eq!(nl.date(thursday(), DateStyle::Long), "8 oktober 2026");
        assert_eq!(nl.date(thursday(), DateStyle::Medium), "8 okt 2026");
        assert_eq!(nl.date(thursday(), DateStyle::Short), "08-10-2026");
        assert_eq!(nl.day(thursday(), DayStyle::Long), "donderdag 8 oktober");
        assert_eq!(nl.day(thursday(), DayStyle::Medium), "do 8 okt");
    }

    #[test]
    fn en_gb_writes_every_date_style_as_cldr_does() {
        let en = formatter("en-GB");
        assert_eq!(
            en.date(thursday(), DateStyle::Full),
            "Thursday, 8 October 2026"
        );
        assert_eq!(en.date(thursday(), DateStyle::Long), "8 October 2026");
        assert_eq!(en.date(thursday(), DateStyle::Medium), "8 Oct 2026");
        assert_eq!(en.date(thursday(), DateStyle::Short), "08/10/2026");
        assert_eq!(en.day(thursday(), DayStyle::Long), "Thursday 8 October");
        assert_eq!(en.day(thursday(), DayStyle::Medium), "Thu 8 Oct");
    }

    #[test]
    fn every_time_style_is_cldrs_in_both_locales() {
        let nl = formatter("nl-NL");
        assert_eq!(
            nl.time(thursday(), TimeStyle::Full),
            "14:05:00 Midden-Europese zomertijd"
        );
        assert_eq!(nl.time(thursday(), TimeStyle::Long), "14:05:00 CEST");
        assert_eq!(nl.time(thursday(), TimeStyle::Medium), "14:05:00");
        assert_eq!(nl.time(thursday(), TimeStyle::Short), "14:05");
        let en = formatter("en-GB");
        assert_eq!(
            en.time(thursday(), TimeStyle::Full),
            "14:05:00 Central European Summer Time"
        );
        assert_eq!(en.time(thursday(), TimeStyle::Long), "14:05:00 CEST");
        assert_eq!(en.time(thursday(), TimeStyle::Medium), "14:05:00");
        assert_eq!(en.time(thursday(), TimeStyle::Short), "14:05");
    }

    #[test]
    fn a_date_and_a_time_are_joined_with_the_locales_glue() {
        let nl = formatter("nl-NL");
        assert_eq!(
            nl.date_time(thursday(), DateStyle::Full, TimeStyle::Short),
            "donderdag 8 oktober 2026 om 14:05"
        );
        assert_eq!(
            nl.date_time(thursday(), DateStyle::Medium, TimeStyle::Short),
            "8 okt 2026, 14:05"
        );
        assert_eq!(
            nl.date_time(thursday(), DateStyle::Short, TimeStyle::Short),
            "08-10-2026, 14:05"
        );
        let en = formatter("en-GB");
        assert_eq!(
            en.date_time(thursday(), DateStyle::Long, TimeStyle::Short),
            "8 October 2026 at 14:05"
        );
        assert_eq!(
            en.date_time(thursday(), DateStyle::Medium, TimeStyle::Long),
            "8 Oct 2026, 14:05:00 CEST"
        );
    }

    #[test]
    fn an_instant_either_side_of_a_dst_change_renders_local_time() {
        let nl = formatter("nl-NL");
        let spring_before = utc(2026, 3, 29, 0, 59, 59);
        let spring_after = utc(2026, 3, 29, 1, 0, 0);
        assert_eq!(nl.time(spring_before, TimeStyle::Long), "01:59:59 CET");
        assert_eq!(nl.time(spring_after, TimeStyle::Long), "03:00:00 CEST");
        assert_eq!(
            nl.time(spring_after, TimeStyle::Full),
            "03:00:00 Midden-Europese zomertijd"
        );

        let autumn_before = utc(2026, 10, 25, 0, 59, 59);
        let autumn_after = utc(2026, 10, 25, 1, 0, 0);
        assert_eq!(nl.time(autumn_before, TimeStyle::Long), "02:59:59 CEST");
        assert_eq!(nl.time(autumn_after, TimeStyle::Long), "02:00:00 CET");
        let en = formatter("en-GB");
        assert_eq!(
            en.time(autumn_after, TimeStyle::Full),
            "02:00:00 Central European Standard Time"
        );
        assert_eq!(
            en.date_time(autumn_before, DateStyle::Full, TimeStyle::Short),
            "Sunday, 25 October 2026 at 02:59"
        );
    }

    #[test]
    fn the_repeated_autumn_hour_reads_twice_with_its_zone() {
        let nl = formatter("nl-NL");
        let first = utc(2026, 10, 25, 0, 30, 0);
        let second = utc(2026, 10, 25, 1, 30, 0);
        assert_eq!(
            nl.time(first, TimeStyle::Short),
            nl.time(second, TimeStyle::Short)
        );
        assert_eq!(nl.time(first, TimeStyle::Long), "02:30:00 CEST");
        assert_eq!(nl.time(second, TimeStyle::Long), "02:30:00 CET");
    }

    #[test]
    fn an_evening_in_utc_is_the_next_day_in_amsterdam() {
        let nl = formatter("nl-NL");
        let late = utc(2026, 12, 31, 23, 30, 0);
        assert_eq!(nl.date(late, DateStyle::Long), "1 januari 2027");
        assert_eq!(nl.time(late, TimeStyle::Short), "00:30");
    }

    #[test]
    fn a_calendar_date_is_written_without_a_zone() {
        let nl = formatter("nl-NL");
        let start = NaiveDate::from_ymd_opt(2026, 10, 8).expect("date");
        assert_eq!(
            nl.calendar_date(start, DateStyle::Full),
            "donderdag 8 oktober 2026"
        );
        assert_eq!(nl.calendar_day(start, DayStyle::Medium), "do 8 okt");
    }

    #[test]
    fn a_region_keeps_its_own_patterns() {
        let us = formatter("en");
        assert_eq!(us.date(thursday(), DateStyle::Long), "October 8, 2026");
        let be = formatter("nl-BE");
        assert_eq!(be.date(thursday(), DateStyle::Short), "8/10/2026");
    }

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn relative_phrases_use_cldrs_words_and_plurals_in_nl() {
        let nl = formatter("nl-NL");
        let day = RelativeUnit::Day;
        assert_eq!(nl.relative(-2, day), "eergisteren");
        assert_eq!(nl.relative(-1, day), "gisteren");
        assert_eq!(nl.relative(0, day), "vandaag");
        assert_eq!(nl.relative(1, day), "morgen");
        assert_eq!(nl.relative(2, day), "overmorgen");
        assert_eq!(nl.relative(3, day), "over 3 dagen");
        assert_eq!(nl.relative(-3, day), "3 dagen geleden");
        assert_eq!(nl.relative(0, RelativeUnit::Minute), "binnen een minuut");
        assert_eq!(nl.relative(1, RelativeUnit::Minute), "over 1 minuut");
        assert_eq!(nl.relative(-5, RelativeUnit::Minute), "5 minuten geleden");
        assert_eq!(nl.relative(0, RelativeUnit::Hour), "binnen een uur");
        assert_eq!(nl.relative(3, RelativeUnit::Hour), "over 3 uur");
        assert_eq!(nl.relative(-1, RelativeUnit::Hour), "1 uur geleden");
        assert_eq!(nl.relative(-1, RelativeUnit::Week), "vorige week");
        assert_eq!(nl.relative(2, RelativeUnit::Week), "over 2 weken");
        assert_eq!(nl.relative(1, RelativeUnit::Month), "volgende maand");
        assert_eq!(nl.relative(-4, RelativeUnit::Month), "4 maanden geleden");
        assert_eq!(nl.relative(0, RelativeUnit::Year), "dit jaar");
        assert_eq!(nl.relative(10, RelativeUnit::Year), "over 10 jaar");
        assert_eq!(nl.relative(1_000, day), "over 1.000 dagen");
    }

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn relative_phrases_use_cldrs_words_and_plurals_in_en() {
        let en = formatter("en-GB");
        let day = RelativeUnit::Day;
        assert_eq!(en.relative(-2, day), "2 days ago");
        assert_eq!(en.relative(-1, day), "yesterday");
        assert_eq!(en.relative(0, day), "today");
        assert_eq!(en.relative(1, day), "tomorrow");
        assert_eq!(en.relative(2, day), "in 2 days");
        assert_eq!(en.relative(0, RelativeUnit::Minute), "this minute");
        assert_eq!(en.relative(1, RelativeUnit::Minute), "in 1 minute");
        assert_eq!(en.relative(-5, RelativeUnit::Minute), "5 minutes ago");
        assert_eq!(en.relative(0, RelativeUnit::Hour), "this hour");
        assert_eq!(en.relative(-1, RelativeUnit::Hour), "1 hour ago");
        assert_eq!(en.relative(1, RelativeUnit::Week), "next week");
        assert_eq!(en.relative(-2, RelativeUnit::Week), "2 weeks ago");
        assert_eq!(en.relative(-1, RelativeUnit::Month), "last month");
        assert_eq!(en.relative(3, RelativeUnit::Month), "in 3 months");
        assert_eq!(en.relative(1, RelativeUnit::Year), "next year");
        assert_eq!(en.relative(-2, RelativeUnit::Year), "2 years ago");
        assert_eq!(en.relative(1_000, day), "in 1,000 days");
    }

    #[test]
    fn the_extremes_of_an_amount_are_written_whole() {
        let en = formatter("en-GB");
        assert_eq!(
            en.relative(i64::MIN, RelativeUnit::Day),
            "9,223,372,036,854,775,808 days ago"
        );
    }

    #[test]
    fn a_dst_day_counts_as_one_calendar_day() {
        let nl = formatter("nl-NL");
        // Saturday 23:30 CET to Sunday 23:30 CEST is 23 hours and one calendar day.
        let saturday = utc(2026, 3, 28, 22, 30, 0);
        let sunday = utc(2026, 3, 29, 21, 30, 0);
        assert_eq!(nl.relative_days(sunday, saturday), "morgen");
        // Saturday 23:59 to Sunday 00:01 is two minutes and one calendar day.
        assert_eq!(
            nl.relative_days(utc(2026, 10, 24, 22, 1, 0), utc(2026, 10, 24, 21, 59, 0)),
            "morgen"
        );
        // Sunday 00:30 to Monday 00:30 across the autumn change is 25 hours and one day.
        assert_eq!(
            nl.relative_days(utc(2026, 10, 25, 23, 30, 0), utc(2026, 10, 24, 22, 30, 0)),
            "morgen"
        );
        assert_eq!(
            nl.relative_days(utc(2026, 10, 1, 10, 0, 0), thursday()),
            "7 dagen geleden"
        );
    }

    #[test]
    fn days_between_counts_calendar_days_on_the_zone_s_clock() {
        let nl = formatter("nl-NL");
        // Saturday 23:59 to Sunday 00:01 in Amsterdam: two minutes, one day.
        assert_eq!(
            nl.days_between(utc(2026, 10, 24, 21, 59, 0), utc(2026, 10, 24, 22, 1, 0)),
            -1
        );
        assert_eq!(
            nl.days_between(utc(2026, 10, 24, 10, 0, 0), utc(2026, 10, 24, 20, 0, 0)),
            0
        );
    }

    #[test]
    fn a_language_without_a_vocabulary_has_no_formatter() {
        assert!(DateTimeFormatter::new(&"de".parse().expect("locale"), Amsterdam).is_none());
        assert!(DateTimeFormatter::new(&"xx".parse().expect("locale"), Amsterdam).is_none());
    }

    #[test]
    fn the_iso_formatter_writes_iso_8601() {
        let iso = DateTimeFormatter::iso(&"de".parse().expect("locale"), Amsterdam);
        assert!(!iso.is_localised());
        assert_eq!(iso.date(thursday(), DateStyle::Full), "2026-10-08");
        assert_eq!(iso.day(thursday(), DayStyle::Long), "--10-08");
        assert_eq!(iso.time(thursday(), TimeStyle::Full), "14:05:00+02:00");
        assert_eq!(iso.time(thursday(), TimeStyle::Short), "14:05");
        assert_eq!(
            iso.date_time(thursday(), DateStyle::Medium, TimeStyle::Medium),
            "2026-10-08T14:05:00"
        );
        assert_eq!(iso.relative(3, RelativeUnit::Day), "P3D");
        assert_eq!(iso.relative(-2, RelativeUnit::Week), "-P2W");
        assert_eq!(iso.relative(5, RelativeUnit::Minute), "PT5M");
        assert_eq!(iso.relative(1, RelativeUnit::Month), "P1M");
    }

    #[test]
    fn the_iso_offset_names_utc_and_negative_zones() {
        assert_eq!(iso_offset(0), "Z");
        assert_eq!(iso_offset(-12_600), "-03:30");
        assert_eq!(iso_offset(19_800), "+05:30");
    }

    #[test]
    fn the_last_instant_chrono_holds_is_written_in_utc() {
        let nl = formatter("nl-NL");
        let last = DateTime::<Utc>::MAX_UTC;
        assert_eq!(
            nl.date(last, DateStyle::Short),
            last.to_rfc3339_opts(SecondsFormat::AutoSi, true)
        );
    }

    #[test]
    fn a_leap_second_is_written_in_utc() {
        let nl = formatter("nl-NL");
        let leap = NaiveDate::from_ymd_opt(2016, 12, 31)
            .and_then(|date| date.and_hms_nano_opt(23, 59, 59, 1_500_000_000))
            .expect("leap second")
            .and_utc();
        assert_eq!(nl.time(leap, TimeStyle::Medium), "2016-12-31T23:59:60.500Z");
    }

    #[test]
    fn a_style_is_built_once_and_shared_across_threads() {
        let nl = std::sync::Arc::new(formatter("nl-NL"));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let nl = std::sync::Arc::clone(&nl);
                std::thread::spawn(move || nl.date(thursday(), DateStyle::Medium))
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().expect("thread"), "8 okt 2026");
        }
    }

    #[test]
    fn every_shape_has_its_own_slot() {
        let dates = [
            DateStyle::Full,
            DateStyle::Long,
            DateStyle::Medium,
            DateStyle::Short,
        ];
        let times = [
            TimeStyle::Full,
            TimeStyle::Long,
            TimeStyle::Medium,
            TimeStyle::Short,
        ];
        let mut shapes: Vec<Shape> = dates.map(Shape::Date).to_vec();
        shapes.extend([Shape::Day(DayStyle::Long), Shape::Day(DayStyle::Medium)]);
        shapes.extend(times.map(Shape::Time));
        for date in dates {
            shapes.extend(times.map(|time| Shape::DateTime(date, time)));
        }
        let mut slots: Vec<usize> = shapes.iter().map(|shape| shape.slot()).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..SLOTS).collect::<Vec<_>>());
    }

    proptest! {
        #[test]
        fn no_instant_in_chronos_range_panics(
            seconds in DateTime::<Utc>::MIN_UTC.timestamp()..=DateTime::<Utc>::MAX_UTC.timestamp(),
        ) {
            let at = DateTime::from_timestamp(seconds, 0).expect("in range");
            for tag in ["nl-NL", "en-GB"] {
                let formatter = formatter(tag);
                prop_assert!(!formatter.date_time(at, DateStyle::Full, TimeStyle::Full).is_empty());
                prop_assert!(!formatter.day(at, DayStyle::Medium).is_empty());
                prop_assert!(!formatter.relative_days(at, thursday()).is_empty());
            }
        }

        #[test]
        fn every_instant_in_icus_range_shows_the_zones_wall_clock(
            seconds in utc(1583, 1, 1, 0, 0, 0).timestamp()..=utc(9999, 12, 30, 0, 0, 0).timestamp(),
        ) {
            let at = DateTime::from_timestamp(seconds, 0).expect("in range");
            let local = at.with_timezone(&Amsterdam);
            let nl = formatter("nl-NL");
            prop_assert_eq!(nl.time(at, TimeStyle::Medium), local.format("%H:%M:%S").to_string());
        }
    }
}
