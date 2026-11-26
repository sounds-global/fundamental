//! [`TranslationGap`] and [`NotReadyToPublish`] — what keeps a record's text from going live.

#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::ToString, vec::Vec};
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

use super::{Languages, Locale, Localized};

/// One field's language that keeps a record from being published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranslationGap {
    /// The field has no text, or blank text, in the language.
    Missing {
        /// The field.
        field: &'static str,
        /// The language.
        language: Locale,
    },
    /// The field's text in the language is to check against its other language.
    ToCheck {
        /// The field.
        field: &'static str,
        /// The language.
        language: Locale,
    },
}

impl TranslationGap {
    /// What keeps `field`, holding `text`, from being published in `languages`; empty text keeps nothing back.
    pub fn of<T: ToString>(
        field: &'static str,
        text: &Localized<T>,
        languages: &Languages,
    ) -> Vec<Self> {
        let missing = text.missing(languages);
        if missing.len() == languages.all().count() {
            return Vec::new();
        }
        let missing = missing
            .into_iter()
            .map(|language| Self::Missing { field, language });
        let to_check = text
            .to_check(languages)
            .into_iter()
            .map(|language| Self::ToCheck { field, language });
        missing.chain(to_check).collect()
    }

    /// The field.
    pub fn field(&self) -> &'static str {
        match self {
            Self::Missing { field, .. } | Self::ToCheck { field, .. } => field,
        }
    }

    /// The language.
    pub fn language(&self) -> &Locale {
        match self {
            Self::Missing { language, .. } | Self::ToCheck { language, .. } => language,
        }
    }
}

/// A record refused publishing: a field's text is missing a language, or has one to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotReadyToPublish {
    record: &'static str,
    gaps: Vec<TranslationGap>,
}

impl NotReadyToPublish {
    /// `record`'s gaps, none of them left out.
    pub fn new(record: &'static str, gaps: Vec<TranslationGap>) -> Self {
        Self { record, gaps }
    }

    /// Each field's language that is missing or to check.
    pub fn gaps(&self) -> &[TranslationGap] {
        &self.gaps
    }
}

impl fmt::Display for NotReadyToPublish {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "this {} is not published until every language is there and checked:",
            self.record
        )?;
        for (at, gap) in self.gaps.iter().enumerate() {
            let separator = if at == 0 { " " } else { "; " };
            match gap {
                TranslationGap::Missing { field, language } => {
                    write!(f, "{separator}{field} is missing in {language}")?;
                }
                TranslationGap::ToCheck { field, language } => {
                    write!(f, "{separator}{field} is to check in {language}")?;
                }
            }
        }
        Ok(())
    }
}

impl core::error::Error for NotReadyToPublish {}

impl crate::Error for NotReadyToPublish {
    fn error_code(&self) -> &'static str {
        "I18N_001"
    }

    fn display_message(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }

    fn http_status_code(&self) -> u16 {
        409
    }
}
