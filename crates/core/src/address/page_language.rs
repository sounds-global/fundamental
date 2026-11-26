//! [`PageLanguage`] — the page being rendered, in one of the languages it is published in.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::i18n::Locale;

/// The page being rendered in one language it is published in: what a language switch links
/// to. It carries no words; the switch names the language in its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageLanguage {
    pub(super) locale: Locale,
    pub(super) href: String,
    pub(super) current: bool,
}

impl PageLanguage {
    /// The language.
    #[must_use]
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// The link that reads the page in it and records the choice: `/nl/waarom?lang=nl`.
    #[must_use]
    pub fn href(&self) -> &str {
        &self.href
    }

    /// Whether the page is being rendered in it.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.current
    }
}
