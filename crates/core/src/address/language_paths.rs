//! [`LanguagePaths`] — one page's canonical path in each language it is spelled in.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

use crate::i18n::Locale;

/// The canonical path of one page per language, and the path every other language uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePaths {
    default: String,
    spelled: Vec<(Locale, String)>,
}

impl LanguagePaths {
    /// One path for every language.
    #[must_use]
    pub fn uniform(path: impl Into<String>) -> Self {
        Self {
            default: path.into(),
            spelled: Vec::new(),
        }
    }

    /// `path` in `locale`, every other language keeping what it had.
    #[must_use]
    pub fn with(mut self, locale: Locale, path: impl Into<String>) -> Self {
        let path = path.into();
        match self.spelled.iter_mut().find(|(known, _)| *known == locale) {
            Some((_, known)) => *known = path,
            None => self.spelled.push((locale, path)),
        }
        self
    }

    /// The path in `locale`: its own spelling, or the default.
    #[must_use]
    pub fn path_in(&self, locale: &Locale) -> &str {
        self.spelled
            .iter()
            .find(|(known, _)| known == locale)
            .map_or(self.default.as_str(), |(_, path)| path.as_str())
    }

    /// The path a language with no spelling of its own uses.
    #[must_use]
    pub fn default_path(&self) -> &str {
        &self.default
    }
}

#[cfg(test)]
mod tests {
    use super::LanguagePaths;
    use crate::i18n::Locale;

    fn locale(tag: &str) -> Locale {
        tag.parse().expect("tag")
    }

    #[test]
    fn a_language_without_a_spelling_uses_the_default() {
        let paths = LanguagePaths::uniform("/producten/eiken-tafel")
            .with(locale("en"), "/producten/oak-table");
        assert_eq!(paths.path_in(&locale("en")), "/producten/oak-table");
        assert_eq!(paths.path_in(&locale("de")), "/producten/eiken-tafel");
        let replaced = paths.with(locale("en"), "/producten/oak-desk");
        assert_eq!(replaced.path_in(&locale("en")), "/producten/oak-desk");
    }
}
