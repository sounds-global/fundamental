//! A minimal, dependency-free BCP-47 locale identifier.

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
};
use core::fmt;
use core::str::FromStr;

/// A parsed BCP-47 language tag, restricted to `language[-region]`.
///
/// The language is lowercased and the region uppercased, so `en-us` equals `en-US`.
///
/// ```
/// use fundamental_core::i18n::Locale;
///
/// let l: Locale = "en-US".parse().unwrap();
/// assert_eq!(l.language(), "en");
/// assert_eq!(l.region(), Some("US"));
/// assert_eq!(l.to_string(), "en-US");
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Locale {
    language: String,
    region: Option<String>,
}

impl Locale {
    /// Builds a locale from already-trusted parts, normalising case.
    ///
    /// Prefer [`Locale::from_str`] for untrusted input.
    pub fn new(language: impl AsRef<str>, region: Option<impl AsRef<str>>) -> Self {
        Self {
            language: language.as_ref().to_ascii_lowercase(),
            region: region.map(|r| r.as_ref().to_ascii_uppercase()),
        }
    }

    /// The primary language subtag, lowercased (e.g. `"en"`).
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The region subtag if present, uppercased (e.g. `"US"`).
    pub fn region(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// True when this locale carries no region, i.e. it is language-only.
    pub fn is_language_only(&self) -> bool {
        self.region.is_none()
    }

    /// Returns the language-only form of this locale (`en-US` → `en`).
    pub fn language_fallback(&self) -> Locale {
        Locale {
            language: self.language.clone(),
            region: None,
        }
    }
}

/// Error returned when a string is not a `language[-region]` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleParseError {
    input: String,
}

impl fmt::Display for LocaleParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid locale tag: '{}'", self.input)
    }
}

impl core::error::Error for LocaleParseError {}

impl FromStr for Locale {
    type Err = LocaleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || LocaleParseError {
            input: s.to_string(),
        };
        // Either `-` or `_` separates subtags.
        let mut parts = s.split(['-', '_']);

        let language = parts.next().ok_or_else(err)?;
        if language.is_empty()
            || language.len() > 8
            || !language.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return Err(err());
        }

        let region = match parts.next() {
            None => None,
            Some(r) => {
                let is_alpha2 = r.len() == 2 && r.bytes().all(|b| b.is_ascii_alphabetic());
                let is_num3 = r.len() == 3 && r.bytes().all(|b| b.is_ascii_digit());
                if !is_alpha2 && !is_num3 {
                    return Err(err());
                }
                Some(r.to_owned())
            }
        };

        if parts.next().is_some() {
            return Err(err());
        }

        Ok(Locale::new(language, region.as_deref()))
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.region {
            Some(r) => write!(f, "{}-{}", self.language, r),
            None => f.write_str(&self.language),
        }
    }
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Locale({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_only() {
        let l: Locale = "en".parse().unwrap();
        assert_eq!(l.language(), "en");
        assert_eq!(l.region(), None);
        assert!(l.is_language_only());
        assert_eq!(l.to_string(), "en");
    }

    #[test]
    fn parses_and_normalises_region() {
        let l: Locale = "es_mx".parse().unwrap();
        assert_eq!(l.language(), "es");
        assert_eq!(l.region(), Some("MX"));
        assert_eq!(l.to_string(), "es-MX");
    }

    #[test]
    fn parses_numeric_region() {
        let l: Locale = "es-419".parse().unwrap();
        assert_eq!(l.region(), Some("419"));
        assert_eq!(l.to_string(), "es-419");
    }

    #[test]
    fn language_fallback_drops_region() {
        let l: Locale = "pt-BR".parse().unwrap();
        assert_eq!(l.language_fallback().to_string(), "pt");
    }

    #[test]
    fn rejects_malformed() {
        assert!("".parse::<Locale>().is_err());
        assert!("e1".parse::<Locale>().is_err());
        assert!("en-USA".parse::<Locale>().is_err());
        assert!("en-US-x".parse::<Locale>().is_err());
        assert!("toolonglang".parse::<Locale>().is_err());
    }

    #[test]
    fn case_insensitive_equality() {
        let a: Locale = "EN-us".parse().unwrap();
        let b: Locale = "en-US".parse().unwrap();
        assert_eq!(a, b);
    }
}
