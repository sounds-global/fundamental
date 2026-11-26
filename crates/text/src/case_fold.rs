/// Whether a profile folds case, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum CaseFold {
    /// Case is meaningful: `Acme` and `acme` are different names.
    None,
    /// Case is not meaningful: `Acme` and `acme` are one name, lowercased per character, locale-independent.
    Lowercase,
}

impl CaseFold {
    /// The byte this choice contributes to a profile's fingerprint.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Lowercase => 1,
        }
    }

    /// Apply the fold to `text`.
    #[must_use]
    pub fn apply(self, text: &str) -> String {
        match self {
            Self::None => text.to_owned(),
            Self::Lowercase => text.chars().flat_map(char::to_lowercase).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CaseFold;

    #[test]
    fn none_leaves_case_alone() {
        assert_eq!(CaseFold::None.apply("AcMe"), "AcMe");
    }

    #[test]
    fn lowercase_folds_ascii_and_beyond() {
        assert_eq!(CaseFold::Lowercase.apply("AcMe"), "acme");
        assert_eq!(CaseFold::Lowercase.apply("ÄÖÜ"), "äöü");
    }

    #[test]
    fn lowercase_is_locale_independent() {
        assert_eq!(CaseFold::Lowercase.apply("I"), "i");
    }

    #[test]
    fn each_choice_has_its_own_code() {
        assert_ne!(CaseFold::None.code(), CaseFold::Lowercase.code());
    }
}
