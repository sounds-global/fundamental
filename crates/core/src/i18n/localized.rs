//! [`Localized<T>`] — a field value with per-locale variants.

#[cfg(not(feature = "std"))]
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::translation_check::ToCheck;
use super::{Checked, Languages, Locale, TranslationCheck, TranslationRow};

/// A value carrying a source-locale form plus sparse per-locale overrides.
///
/// ```
/// use fundamental_core::i18n::Localized;
///
/// let mut name: Localized<String> = "Wireless Headphones".into();
/// name.set("es", "Auriculares inalámbricos");
///
/// let es: fundamental_core::i18n::Locale = "es".parse().unwrap();
/// assert_eq!(name.resolve(&es), "Auriculares inalámbricos");
/// let de: fundamental_core::i18n::Locale = "de".parse().unwrap();
/// assert_eq!(name.resolve(&de), "Wireless Headphones"); // falls back to source
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localized<T> {
    /// The source-locale value — always present, the final fallback.
    source: T,
    /// Overrides keyed by canonical BCP-47 tag; a locale without one falls back.
    #[serde(default = "BTreeMap::new", skip_serializing_if = "BTreeMap::is_empty")]
    translations: BTreeMap<String, T>,
    /// Where each override and the source stand against each other, keyed as the overrides are.
    #[serde(default = "BTreeMap::new", skip_serializing_if = "BTreeMap::is_empty")]
    checks: BTreeMap<String, TranslationCheck>,
}

impl<T> Localized<T> {
    /// Creates a value with only its source-locale form.
    pub fn new(source: impl Into<T>) -> Self {
        Self {
            source: source.into(),
            translations: BTreeMap::new(),
            checks: BTreeMap::new(),
        }
    }

    /// The source-locale value (the final fallback).
    pub fn source(&self) -> &T {
        &self.source
    }

    /// Puts back an override as it was stored, with where it and the source stood.
    pub fn restore(
        &mut self,
        locale: &str,
        value: impl Into<T>,
        check: TranslationCheck,
    ) -> &mut Self {
        let tag = canonical(locale);
        self.translations.insert(tag.clone(), value.into());
        self.checks.insert(tag, check);
        self
    }

    /// The exact override for `locale`, if one exists (no fallback).
    pub fn get(&self, locale: &Locale) -> Option<&T> {
        self.translations.get(&locale.to_string())
    }

    /// Resolves `locale` to a value, falling back `locale → language-only → source`.
    pub fn resolve(&self, locale: &Locale) -> &T {
        if let Some(v) = self.translations.get(&locale.to_string()) {
            return v;
        }
        if !locale.is_language_only()
            && let Some(v) = self
                .translations
                .get(&locale.language_fallback().to_string())
        {
            return v;
        }
        &self.source
    }

    /// Resolves against the ambient translator's locale chain, falling back to the source value.
    #[cfg_attr(
        feature = "std",
        expect(clippy::excessive_nesting, reason = "readability baseline")
    )]
    pub fn current(&self) -> &T {
        #[cfg(feature = "std")]
        {
            if let Some(translator) = super::current() {
                for locale in translator.locale_chain() {
                    if let Some(v) = self.translations.get(&locale.to_string()) {
                        return v;
                    }
                }
            }
        }
        &self.source
    }

    /// Iterates the stored overrides as `(canonical_tag, value)` pairs.
    pub fn translations(&self) -> impl Iterator<Item = (&str, &T)> {
        self.translations.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// True when at least one locale override is stored (beyond the source form).
    pub fn is_translated(&self) -> bool {
        !self.translations.is_empty()
    }

    /// The locales (canonical BCP-47 tags) that have an explicit override, the source excluded.
    pub fn translated_locales(&self) -> impl Iterator<Item = &str> {
        self.translations.keys().map(String::as_str)
    }

    /// Removes the override for `locale` and its check; answers whether there was one.
    pub fn remove(&mut self, locale: &Locale) -> bool {
        let tag = locale.to_string();
        self.checks.remove(&tag);
        self.translations.remove(&tag).is_some()
    }
}

impl<T: ToString> Localized<T> {
    /// The overrides as their companion rows store them under `field`, settled as the texts read now.
    pub fn translation_rows(&self, field: &str) -> Vec<TranslationRow> {
        let source = self.source.to_string();
        self.translations
            .iter()
            .map(|(tag, value)| {
                let value = value.to_string();
                let mut check = self.checks.get(tag).cloned().unwrap_or_default();
                check.settle(&source, &value);
                TranslationRow {
                    field: field.to_string(),
                    locale: tag.clone(),
                    value,
                    check,
                }
            })
            .collect()
    }

    /// The languages of `languages` this value has no text in, the source included.
    pub fn missing(&self, languages: &Languages) -> Vec<Locale> {
        languages
            .all()
            .filter(|language| !self.has_text(language, languages))
            .cloned()
            .collect()
    }

    fn has_text(&self, language: &Locale, languages: &Languages) -> bool {
        let text = if language == languages.source() {
            Some(&self.source)
        } else {
            self.translations.get(&language.to_string())
        };
        text.is_some_and(|text| !text.to_string().trim().is_empty())
    }

    /// Replaces the source-locale value, marking translations that fitted it to check.
    pub fn set_source(&mut self, value: impl Into<T>) -> &mut Self {
        let value = value.into();
        let (before, after) = (self.source.to_string(), value.to_string());
        for (tag, translation) in &self.translations {
            let translation = translation.to_string();
            let check = self.checks.entry(tag.clone()).or_default();
            check.settle(&before, &translation);
            if check.to_check(&before, &translation) == ToCheck::Source {
                *check = TranslationCheck::fitting(&after, &translation);
            }
        }
        self.source = value;
        self
    }

    /// Sets the override for `locale` (a BCP-47 tag; `es_mx` → `es-MX`).
    pub fn set(&mut self, locale: &str, value: impl Into<T>) -> &mut Self {
        let value = value.into();
        let tag = canonical(locale);
        let source = self.source.to_string();
        let after = value.to_string();
        let check = match self.translations.get(&tag) {
            None => TranslationCheck::fitting(&source, &after),
            Some(before) => {
                let before = before.to_string();
                let mut check = self.checks.get(&tag).cloned().unwrap_or_default();
                check.settle(&source, &before);
                if check.to_check(&source, &before) == ToCheck::Translation {
                    check = TranslationCheck::fitting(&source, &after);
                }
                check
            }
        };
        self.translations.insert(tag.clone(), value);
        self.checks.insert(tag, check);
        self
    }

    /// Builder form of [`set`](Self::set).
    #[must_use]
    pub fn with(mut self, locale: &str, value: impl Into<T>) -> Self {
        self.set(locale, value);
        self
    }

    /// Confirms `language`'s text still fits the other language it was to check against, by whom
    /// and when; answers whether anything was to check.
    pub fn still_fits(
        &mut self,
        language: &Locale,
        languages: &Languages,
        checked: Checked,
    ) -> bool {
        let source = self.source.to_string();
        let (side, tags): (ToCheck, Vec<String>) = if language == languages.source() {
            (
                ToCheck::Source,
                languages.others().iter().map(ToString::to_string).collect(),
            )
        } else if languages.others().contains(language) {
            (ToCheck::Translation, Vec::from([language.to_string()]))
        } else {
            return false;
        };
        let mut confirmed = false;
        for tag in tags {
            let (Some(translation), Some(check)) =
                (self.translations.get(&tag), self.checks.get_mut(&tag))
            else {
                continue;
            };
            let translation = translation.to_string();
            if check.to_check(&source, &translation) == side {
                check.confirm(&source, &translation, checked.clone());
                confirmed = true;
            }
        }
        confirmed
    }

    /// Who last confirmed the text in `translation` and the source still fit each other, and when.
    pub fn checked(&self, translation: &Locale) -> Option<&Checked> {
        self.checks
            .get(&translation.to_string())
            .and_then(TranslationCheck::checked)
    }

    /// The languages of `languages` whose text is to check; a missing language is not to check.
    pub fn to_check(&self, languages: &Languages) -> Vec<Locale> {
        let source = self.source.to_string();
        let mut source_to_check = false;
        let mut to_check = Vec::new();
        for language in languages.others() {
            if !self.has_text(language, languages) {
                continue;
            }
            let tag = language.to_string();
            let (Some(translation), Some(check)) =
                (self.translations.get(&tag), self.checks.get(&tag))
            else {
                continue;
            };
            match check.to_check(&source, &translation.to_string()) {
                ToCheck::Translation => to_check.push(language.clone()),
                ToCheck::Source => source_to_check = true,
                ToCheck::Neither => {}
            }
        }
        if source_to_check && self.has_text(languages.source(), languages) {
            to_check.insert(0, languages.source().clone());
        }
        to_check
    }
}

/// Normalizes a BCP-47 tag to its canonical form, leaving an unparseable tag as given.
fn canonical(locale: &str) -> String {
    locale
        .parse::<Locale>()
        .map_or_else(|_| locale.to_string(), |l| l.to_string())
}

/// A stable FNV-1a fingerprint (16-hex) of a source value, matching the build tooling's `content_hash`.
pub fn source_fingerprint(source: &str) -> String {
    use core::fmt::Write as _;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut out = String::new();
    let _ = write!(out, "{hash:016x}");
    out
}

impl<T: fmt::Display> fmt::Display for Localized<T> {
    /// Renders the value in the ambient locale (see [`current`](Self::current)).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.current().fmt(f)
    }
}

impl From<&str> for Localized<String> {
    fn from(source: &str) -> Self {
        Localized::new(source.to_string())
    }
}

impl From<String> for Localized<String> {
    fn from(source: String) -> Self {
        Localized::new(source)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::i18n::{Arg, Translator, with};
    use std::sync::Arc;

    #[test]
    fn resolves_with_region_fallback() {
        let mut v: Localized<String> = "color".into();
        v.set("es", "color-es").set("en-GB", "colour");

        let es_mx: Locale = "es-MX".parse().unwrap();
        assert_eq!(v.resolve(&es_mx), "color-es"); // es-MX → es
        let en_gb: Locale = "en-GB".parse().unwrap();
        assert_eq!(v.resolve(&en_gb), "colour"); // exact
        let de: Locale = "de".parse().unwrap();
        assert_eq!(v.resolve(&de), "color"); // → source

        assert_eq!(v.get(&es_mx), None); // get is exact, no fallback
    }

    struct EsMx;
    impl Translator for EsMx {
        fn lookup(&self, _k: &str, _a: &[Arg<'_>]) -> Option<String> {
            None
        }
        fn locale_tag(&self) -> &str {
            "es-MX"
        }
    }

    fn languages(source: &str, others: &[&str]) -> Languages {
        Languages::new(
            source.parse().expect("a language"),
            others.iter().map(|tag| tag.parse().expect("a language")),
        )
    }

    fn tags(locales: Vec<Locale>) -> Vec<String> {
        locales.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_language_without_text_or_with_blank_text_is_missing_the_source_too() {
        let both = languages("en", &["nl", "de"]);
        let v: Localized<String> = Localized::new("Hi").with("nl", "Hoi").with("de", "  ");
        assert_eq!(tags(v.missing(&both)), ["de"], "blank text is missing text");

        let untitled: Localized<String> = Localized::new("").with("nl", "Hoi");
        assert_eq!(
            tags(untitled.missing(&languages("en", &["nl"]))),
            ["en"],
            "the source is a language like the others"
        );
        assert!(v.missing(&languages("en", &["nl"])).is_empty());
    }

    #[test]
    fn editing_either_language_marks_the_other_to_check() {
        let both = languages("en", &["nl"]);
        let mut v: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        assert!(
            v.to_check(&both).is_empty(),
            "a new translation fits its source"
        );

        v.set_source("Open now");
        assert_eq!(tags(v.to_check(&both)), ["nl"], "the English changed");
        v.set_source("Open today");
        assert_eq!(tags(v.to_check(&both)), ["nl"], "and changed again");

        v.set("nl", "Vandaag open");
        assert!(
            v.to_check(&both).is_empty(),
            "editing the Dutch that was to check answers the English"
        );

        v.set("nl", "Vandaag geopend");
        assert_eq!(
            tags(v.to_check(&both)),
            ["en"],
            "the Dutch changed on its own"
        );
        v.set_source("Opened today");
        assert!(v.to_check(&both).is_empty(), "the English answers it");
    }

    #[test]
    fn a_text_put_back_as_it_was_is_no_longer_to_check() {
        let both = languages("en", &["nl"]);
        let mut v: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        v.set_source("Closed");
        v.set_source("Open");
        assert!(
            v.to_check(&both).is_empty(),
            "an undone change leaves nothing to check"
        );
    }

    #[test]
    fn a_value_replaced_whole_with_both_languages_changed_leaves_neither_to_check() {
        let both = languages("en", &["nl"]);
        let stored: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        // What a form posts back: the value as it was read, its texts replaced.
        let mut posted = serde_json::to_value(&stored).expect("serialised");
        posted["source"] = "Open now".into();
        posted["translations"]["nl"] = "Nu open".into();
        let posted: Localized<String> = serde_json::from_value(posted).expect("read back");
        assert!(
            posted.to_check(&both).is_empty(),
            "both were written together"
        );

        let mut english_only = serde_json::to_value(&stored).expect("serialised");
        english_only["source"] = "Open now".into();
        let english_only: Localized<String> =
            serde_json::from_value(english_only).expect("read back");
        assert_eq!(tags(english_only.to_check(&both)), ["nl"]);
    }

    #[test]
    fn a_missing_language_is_missing_not_to_check() {
        let three = languages("en", &["nl", "de"]);
        let mut v: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        v.set_source("Open now");
        assert_eq!(tags(v.to_check(&three)), ["nl"]);
        assert_eq!(tags(v.missing(&three)), ["de"]);
    }

    #[test]
    fn still_fits_clears_a_language_to_check_with_who_and_when() {
        let both = languages("en", &["nl"]);
        let (en, nl): (Locale, Locale) = ("en".parse().unwrap(), "nl".parse().unwrap());
        let mut v: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        assert!(
            !v.still_fits(&nl, &both, Checked::new("ann", 1)),
            "nothing to check, nothing confirmed"
        );
        assert_eq!(v.checked(&nl), None);

        v.set_source("Open now");
        assert!(v.still_fits(&nl, &both, Checked::new("ann", 1_000)));
        assert!(v.to_check(&both).is_empty(), "the Dutch still fits");
        assert_eq!(v.checked(&nl), Some(&Checked::new("ann", 1_000)));

        v.set("nl", "Nu geopend");
        assert_eq!(tags(v.to_check(&both)), ["en"]);
        assert!(v.still_fits(&en, &both, Checked::new("bo", 2_000)));
        assert!(v.to_check(&both).is_empty(), "the English still fits");
        assert_eq!(v.checked(&nl), Some(&Checked::new("bo", 2_000)));

        v.set("nl", "Geopend");
        v.set_source("Opened");
        assert!(v.to_check(&both).is_empty(), "an edit answering the other");
        assert_eq!(
            v.checked(&nl),
            None,
            "fits by the edit, not by a confirmation"
        );
    }

    #[test]
    fn a_removed_translation_is_missing_and_read_from_the_source() {
        let both = languages("en", &["nl"]);
        let nl: Locale = "nl".parse().unwrap();
        let mut v: Localized<String> = Localized::new("Open").with("nl", "Geopend");
        v.set_source("Open now");

        assert!(v.remove(&nl), "there was Dutch to remove");
        assert_eq!(v.get(&nl), None);
        assert_eq!(v.resolve(&nl), "Open now");
        assert_eq!(tags(v.missing(&both)), ["nl"]);
        assert!(v.to_check(&both).is_empty(), "nothing is left to check");
        assert!(!v.remove(&nl), "nothing left to remove");
    }

    #[test]
    fn display_uses_ambient_locale_chain() {
        let v: Localized<String> = Localized::new("Cancel").with("es", "Cancelar");
        assert_eq!(v.to_string(), "Cancel"); // no translator → source
        let rendered = with(Arc::new(EsMx), || v.to_string()); // es-MX → es override
        assert_eq!(rendered, "Cancelar");
    }
}
