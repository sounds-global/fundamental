//! The messages an app is built from, and the languages its pages render in.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono_tz::Tz;
use fundamental_core::i18n::{Locale, negotiate_chain};
use fundamental_messages::Catalogs;
use tracing::{debug, info, instrument, warn};

use crate::DateTimeFormatter;

/// The compiled messages of an app and of the crates it is built from, and its source locale.
#[derive(Clone)]
pub struct Catalog {
    pub(crate) inner: Arc<CatalogInner>,
}

pub(crate) struct CatalogInner {
    pub(crate) source_locale: Locale,
    /// The MessageFormat 2 messages of the crates the app is built from.
    pub(crate) messages: Vec<&'static Catalogs>,
    /// The app's own MessageFormat 2 messages, which win over a crate's of the same id.
    pub(crate) own: Vec<&'static Catalogs>,
    /// Every language a page can render in.
    languages: HashSet<Locale>,
    /// One flag per locale, set the first time dates in it fall back.
    date_fallback_reported: HashMap<Locale, AtomicBool>,
}

impl Catalog {
    /// Starts building a catalog whose authoritative language is `source_locale`.
    pub fn builder(source_locale: Locale) -> CatalogBuilder {
        CatalogBuilder {
            source_locale,
            messages: Vec::new(),
            own: Vec::new(),
        }
    }

    /// The authoritative source locale, the bottom of every fallback chain.
    pub fn source_locale(&self) -> &Locale {
        &self.inner.source_locale
    }

    /// True when a page can render in `locale`: the source, or a language the registered messages speak.
    pub fn has_locale(&self, locale: &Locale) -> bool {
        self.inner.languages.contains(locale)
    }

    /// A [`DateTimeFormatter`] for `requested` on `zone`'s wall clock, in the page's language.
    ///
    /// Falls back to the source locale, then ISO 8601, with a `warn!` once per locale.
    #[must_use]
    #[instrument(level = "trace", skip(self), fields(requested = %requested, zone = zone.name()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn date_time_formatter(&self, requested: &Locale, zone: Tz) -> DateTimeFormatter {
        let source = self.source_locale();
        let chain = negotiate_chain(requested, &[], source);
        let rendered = chain
            .iter()
            .find(|locale| self.has_locale(locale))
            .unwrap_or(source);
        // A bare language names no region, so the catalog's region stands: `en` on `en-GB` reads British.
        let writes_in =
            if requested.region().is_some() && requested.language() == rendered.language() {
                requested
            } else {
                rendered
            };
        if let Some(formatter) = DateTimeFormatter::new(writes_in, zone) {
            debug!(requested = %requested, writes_in = %writes_in, zone = zone.name(), "negotiated date formatting locale");
            return formatter;
        }
        self.report_date_fallback(rendered, source);
        if rendered != source {
            if let Some(formatter) = DateTimeFormatter::new(source, zone) {
                return formatter;
            }
            self.report_date_fallback(source, source);
        }
        DateTimeFormatter::iso(source, zone)
    }

    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn report_date_fallback(&self, locale: &Locale, source: &Locale) {
        let first = self
            .inner
            .date_fallback_reported
            .get(locale)
            .is_some_and(|reported| !reported.swap(true, Ordering::Relaxed));
        if first {
            if locale == source {
                warn!(locale = %locale, "the catalog's source locale has no date vocabulary; writing dates in ISO 8601");
            } else {
                warn!(locale = %locale, source = %source, "catalog locale has no date vocabulary; writing its dates in the source locale");
            }
        }
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut locales: Vec<String> = self.inner.languages.iter().map(Locale::to_string).collect();
        locales.sort();
        f.debug_struct("Catalog")
            .field("source_locale", &self.inner.source_locale.to_string())
            .field("locales", &locales)
            .finish()
    }
}

/// Assembles a [`Catalog`] from the app's messages and each crate's.
pub struct CatalogBuilder {
    source_locale: Locale,
    messages: Vec<&'static Catalogs>,
    own: Vec<&'static Catalogs>,
}

impl CatalogBuilder {
    /// Adds a crate's MessageFormat 2 messages (`m::CATALOGS`); between two crates the first added stands.
    pub fn with_messages(mut self, catalogs: &'static Catalogs) -> Self {
        self.messages.push(catalogs);
        self
    }

    /// Adds the app's own MessageFormat 2 messages (`crate::m::CATALOGS`), which win over any crate's.
    pub fn with_own_messages(mut self, catalogs: &'static Catalogs) -> Self {
        self.own.push(catalogs);
        self
    }

    /// The catalog: every language the registered messages speak, and the source.
    pub fn build(self) -> Catalog {
        let mut languages: HashSet<Locale> = self
            .messages
            .iter()
            .chain(&self.own)
            .flat_map(|catalogs| catalogs.languages())
            .filter_map(|tag| tag.parse().ok())
            .collect();
        languages.insert(self.source_locale.clone());
        // Framework words come after every crate's, so an app or library can word the same id its own way.
        let mut messages = self.messages;
        messages.push(&crate::m::CATALOGS);
        info!(
            source_locale = %self.source_locale,
            languages = languages.len(),
            crates = messages.len(),
            own = self.own.len(),
            "translation catalog built"
        );
        let date_fallback_reported = languages
            .iter()
            .map(|locale| (locale.clone(), AtomicBool::new(false)))
            .collect();
        Catalog {
            inner: Arc::new(CatalogInner {
                source_locale: self.source_locale,
                messages,
                own: self.own,
                languages,
                date_fallback_reported,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use chrono_tz::Europe::Amsterdam;
    use tracing_test::traced_test;

    use super::*;
    use crate::DateStyle;

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("valid locale")
    }

    /// Catalogs that speak each of `languages` and say nothing.
    macro_rules! speaking {
        ($($tag:literal),+) => {{
            static SPEAKING: Catalogs = Catalogs::new("en", 0, &[$(($tag, b"", b"")),+]);
            &SPEAKING
        }};
    }

    fn catalog(source: &str, speaking: &'static Catalogs) -> Catalog {
        Catalog::builder(locale(source))
            .with_messages(speaking)
            .build()
    }

    fn thursday() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 8, 12, 5, 0)
            .single()
            .expect("valid instant")
    }

    fn fallback_warnings(lines: &[&str]) -> usize {
        lines
            .iter()
            .filter(|line| line.contains("has no date vocabulary"))
            .count()
    }

    #[test]
    fn dates_are_written_in_the_language_the_page_renders_in() {
        let catalog = catalog("en", speaking!("en", "nl"));
        let formatter = catalog.date_time_formatter(&locale("nl-NL"), Amsterdam);
        assert_eq!(formatter.locale(), &locale("nl-NL"));
        assert_eq!(
            formatter.date(thursday(), DateStyle::Long),
            "8 oktober 2026"
        );
    }

    #[test]
    fn a_readers_region_is_kept_on_a_page_in_its_language() {
        let catalog = catalog("en", speaking!("en", "nl"));
        let formatter = catalog.date_time_formatter(&locale("nl-BE"), Amsterdam);
        assert_eq!(formatter.date(thursday(), DateStyle::Short), "8/10/2026");
    }

    #[test]
    fn a_reader_who_names_no_region_reads_the_catalogs() {
        let catalog = catalog("en-GB", speaking!("en-GB", "nl"));
        let formatter = catalog.date_time_formatter(&locale("en"), Amsterdam);
        assert_eq!(formatter.locale(), &locale("en-GB"));
        assert_eq!(
            formatter.date(thursday(), DateStyle::Long),
            "8 October 2026"
        );
    }

    #[test]
    fn a_language_the_catalog_lacks_reads_the_source_locales_dates() {
        let catalog = catalog("en-GB", speaking!("en-GB", "nl"));
        let formatter = catalog.date_time_formatter(&locale("fr"), Amsterdam);
        assert_eq!(formatter.locale(), &locale("en-GB"));
        assert_eq!(
            formatter.date(thursday(), DateStyle::Long),
            "8 October 2026"
        );
    }

    #[traced_test]
    #[test]
    fn a_catalog_locale_without_date_vocabulary_falls_back_to_the_source_logged_once() {
        let catalog = catalog("nl", speaking!("nl", "de"));
        for _ in 0..3 {
            let formatter = catalog.date_time_formatter(&locale("de"), Amsterdam);
            assert!(formatter.is_localised());
            assert_eq!(formatter.locale(), &locale("nl"));
            assert_eq!(
                formatter.date(thursday(), DateStyle::Long),
                "8 oktober 2026"
            );
        }
        logs_assert(|lines| match fallback_warnings(lines) {
            1 => Ok(()),
            count => Err(format!("expected one fallback warning, saw {count}")),
        });
    }

    #[traced_test]
    #[test]
    fn a_source_locale_without_date_vocabulary_writes_iso_logged_once() {
        let catalog = catalog("de", speaking!("de", "fr"));
        for requested in ["de", "fr", "de-AT"] {
            let formatter = catalog.date_time_formatter(&locale(requested), Amsterdam);
            assert!(!formatter.is_localised());
            assert_eq!(formatter.date(thursday(), DateStyle::Full), "2026-10-08");
        }
        logs_assert(|lines| match fallback_warnings(lines) {
            2 => Ok(()),
            count => Err(format!(
                "expected one warning for de, the source, and one for fr, saw {count}"
            )),
        });
    }

    #[test]
    fn a_stream_of_unknown_languages_adds_no_state() {
        let catalog = catalog("en", speaking!("en"));
        let before = catalog.inner.date_fallback_reported.len();
        for tag in ["aa", "ab", "af", "ak", "am", "an", "ar", "as", "av", "ay"] {
            let formatter = catalog.date_time_formatter(&locale(tag), Amsterdam);
            assert_eq!(formatter.locale(), &locale("en"));
        }
        assert_eq!(catalog.inner.date_fallback_reported.len(), before);
    }
}
