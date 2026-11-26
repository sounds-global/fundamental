//! [`Languages`] — the languages a stored text is kept in.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::Locale;

/// The languages a stored text is kept in: its source and every other.
///
/// ```
/// use fundamental_core::i18n::{Languages, Locale};
///
/// let en: Locale = "en".parse().unwrap();
/// let nl: Locale = "nl".parse().unwrap();
/// let languages = Languages::new(en.clone(), [nl.clone(), en.clone()]);
/// assert_eq!(languages.source(), &en);
/// assert_eq!(languages.others(), [nl]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Languages {
    source: Locale,
    others: Vec<Locale>,
}

impl Languages {
    /// The source language and the others, each kept once.
    pub fn new(source: Locale, others: impl IntoIterator<Item = Locale>) -> Self {
        let mut kept: Vec<Locale> = Vec::new();
        for language in others {
            if language != source && !kept.contains(&language) {
                kept.push(language);
            }
        }
        Self {
            source,
            others: kept,
        }
    }

    /// The language a text is written in first.
    pub fn source(&self) -> &Locale {
        &self.source
    }

    /// Every other language a text must be read in, in the order given.
    pub fn others(&self) -> &[Locale] {
        &self.others
    }

    /// Every language, the source first.
    pub fn all(&self) -> impl Iterator<Item = &Locale> {
        core::iter::once(&self.source).chain(self.others.iter())
    }
}
