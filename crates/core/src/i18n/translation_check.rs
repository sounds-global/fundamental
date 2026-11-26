//! [`TranslationCheck`] — where a translation and its source stand against each other.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use serde::{Deserialize, Serialize};

use super::Checked;

/// Where one translation and its source stand: each keeps the fingerprint of the other's text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationCheck {
    /// The source's fingerprint when the translation last fitted it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) source_hash: String,
    /// The translation's fingerprint when the source last fitted it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) translation_hash: String,
    /// Who confirmed the side that was to check still fits, and when.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checked: Option<Checked>,
}

/// Which side of a translation and its source is to check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToCheck {
    Neither,
    Translation,
    Source,
}

impl TranslationCheck {
    /// Both sides fitting each other as they now read.
    pub(crate) fn fitting(source: &str, translation: &str) -> Self {
        Self {
            source_hash: super::source_fingerprint(source),
            translation_hash: super::source_fingerprint(translation),
            checked: None,
        }
    }

    /// The side that was to check confirmed as fitting the other, by whom and when.
    pub(crate) fn confirm(&mut self, source: &str, translation: &str, checked: Checked) {
        *self = Self {
            checked: Some(checked),
            ..Self::fitting(source, translation)
        };
    }

    /// Which side is to check: the one whose counterpart moved while it stayed.
    pub(crate) fn to_check(&self, source: &str, translation: &str) -> ToCheck {
        match self.moved(source, translation) {
            (true, false) => ToCheck::Translation,
            (false, true) => ToCheck::Source,
            _ => ToCheck::Neither,
        }
    }

    /// Records what is not known and settles two sides written together, as the texts read now.
    pub(crate) fn settle(&mut self, source: &str, translation: &str) {
        let both_moved = self.moved(source, translation) == (true, true);
        if self.source_hash.is_empty() || both_moved {
            self.source_hash = super::source_fingerprint(source);
        }
        if self.translation_hash.is_empty() || both_moved {
            self.translation_hash = super::source_fingerprint(translation);
        }
        if both_moved {
            self.checked = None;
        }
    }

    /// Whether the source and the translation each moved from the text the other last fitted.
    fn moved(&self, source: &str, translation: &str) -> (bool, bool) {
        let moved =
            |hash: &str, text: &str| !hash.is_empty() && hash != super::source_fingerprint(text);
        (
            moved(&self.source_hash, source),
            moved(&self.translation_hash, translation),
        )
    }
}

impl TranslationCheck {
    /// A check as it was stored, each fingerprint `""` when not recorded.
    pub fn stored(
        source_hash: impl Into<String>,
        translation_hash: impl Into<String>,
        checked: Option<Checked>,
    ) -> Self {
        Self {
            source_hash: source_hash.into(),
            translation_hash: translation_hash.into(),
            checked,
        }
    }

    /// The source's fingerprint when the translation last fitted it, `""` when not recorded.
    pub fn source_hash(&self) -> &str {
        &self.source_hash
    }

    /// The translation's fingerprint when the source last fitted it, `""` when not recorded.
    pub fn translation_hash(&self) -> &str {
        &self.translation_hash
    }

    /// Who last confirmed one side still fits the other, and when.
    pub fn checked(&self) -> Option<&Checked> {
        self.checked.as_ref()
    }
}

/// One stored translation of a model's `Localized` field, as its companion table holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslationRow {
    /// The field's column.
    pub field: String,
    /// The language, a canonical BCP-47 tag.
    pub locale: String,
    /// The text in that language.
    pub value: String,
    /// Where the text and the source stand against each other.
    pub check: TranslationCheck,
}
