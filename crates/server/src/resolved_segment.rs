use fundamental_core::i18n::Locale;

/// A path segment resolved to its record: its id, its canonical spelling, and each language's spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSegment {
    id: String,
    canonical: String,
    spellings: Vec<(Locale, String)>,
    default_spelling: Option<String>,
}

impl ResolvedSegment {
    /// The longest canonical spelling a segment may have.
    pub const MAX_LEN: usize = 128;

    /// `id`, canonically spelled `canonical`; `None` unless `canonical` is 1–128 RFC 3986 unreserved characters, not `.` or `..`.
    #[must_use]
    pub fn new(id: impl Into<String>, canonical: impl Into<String>) -> Option<Self> {
        let canonical = canonical.into();
        is_segment(&canonical).then(|| Self {
            id: id.into(),
            canonical,
            spellings: Vec::new(),
            default_spelling: None,
        })
    }

    /// The spelling every language without its own uses.
    ///
    /// `None` when `segment` is not a valid segment, as for [`new`](Self::new).
    #[must_use]
    pub fn with_default_spelling(mut self, segment: impl Into<String>) -> Option<Self> {
        let segment = segment.into();
        if !is_segment(&segment) {
            return None;
        }
        self.default_spelling = Some(segment);
        Some(self)
    }

    /// The segment as `locale` spells it.
    ///
    /// `None` when `segment` is not a valid segment, as for [`new`](Self::new).
    #[must_use]
    pub fn with_spelling(mut self, locale: Locale, segment: impl Into<String>) -> Option<Self> {
        let segment = segment.into();
        if !is_segment(&segment) {
            return None;
        }
        self.spellings.push((locale, segment));
        Some(self)
    }

    /// The record's id, as the route's handlers read it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The canonical spelling.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// The spelling in `locale`'s language: its own, the default, or the canonical one.
    #[must_use]
    pub fn spelling_in(&self, locale: &Locale) -> &str {
        self.own_spelling(locale)
            .unwrap_or_else(|| self.default_spelling())
    }

    /// The spelling a language without its own uses.
    #[must_use]
    pub fn default_spelling(&self) -> &str {
        self.default_spelling.as_deref().unwrap_or(&self.canonical)
    }

    fn own_spelling(&self, locale: &Locale) -> Option<&str> {
        self.spellings
            .iter()
            .find(|(known, _)| known.language().eq_ignore_ascii_case(locale.language()))
            .map(|(_, segment)| segment.as_str())
    }

    /// The languages with a spelling of their own.
    pub fn spelled_languages(&self) -> impl Iterator<Item = &Locale> {
        self.spellings.iter().map(|(locale, _)| locale)
    }
}

fn is_segment(text: &str) -> bool {
    (1..=ResolvedSegment::MAX_LEN).contains(&text.len())
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

#[cfg(test)]
mod tests {
    use super::ResolvedSegment;

    #[test]
    fn a_canonical_spelling_can_carry_no_separator() {
        for refused in ["", ".", "..", "a/b", "a?b", "a#b", "a%2Fb", "a b", "é"] {
            assert!(ResolvedSegment::new("id", refused).is_none(), "{refused:?}");
        }
        let long = "a".repeat(ResolvedSegment::MAX_LEN + 1);
        assert!(ResolvedSegment::new("id", long).is_none());
        assert!(ResolvedSegment::new("id", "team_01h455vb4pex5vsknk084sn02q").is_some());
    }

    #[test]
    fn a_language_without_its_own_spelling_uses_the_canonical_one() {
        let english = "en".parse().expect("tag");
        let segment = ResolvedSegment::new("7", "eiken-tafel")
            .and_then(|segment| segment.with_spelling(english, "oak-table"));
        let segment = segment.expect("valid spellings");
        assert_eq!(
            segment.spelling_in(&"en".parse().expect("tag")),
            "oak-table"
        );
        assert_eq!(
            segment.spelling_in(&"de".parse().expect("tag")),
            "eiken-tafel"
        );
        assert_eq!(segment.id(), "7");
    }
}
