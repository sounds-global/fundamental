#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::String};
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

/// Which provider answered behind a port (`lettermint`, `kvk`): a slug of at most 32 characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderName(Cow<'static, str>);

impl ProviderName {
    /// The name `slug`, which an adapter declares as a constant.
    ///
    /// # Panics
    /// When `slug` is not a lower-case letter followed by up to 31 lower-case letters, digits or `_`.
    #[must_use]
    pub const fn new(slug: &'static str) -> Self {
        assert!(
            is_slug(slug.as_bytes()),
            "a provider name is a lower-case slug"
        );
        Self(Cow::Borrowed(slug))
    }

    /// The name `slug` chosen at run time, or `None` when it is not a slug.
    #[must_use]
    pub fn owned(slug: String) -> Option<Self> {
        is_slug(slug.as_bytes()).then_some(Self(Cow::Owned(slug)))
    }

    /// The slug.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

const fn is_slug(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 32 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{ProviderName, is_slug};

    #[test]
    fn a_provider_name_is_a_slug() {
        assert_eq!(ProviderName::new("lettermint").as_str(), "lettermint");
        assert!(!is_slug(b"Lettermint"));
        assert!(!is_slug(b""));
        assert!(!is_slug(b"let-ter"));
    }

    #[test]
    fn a_name_chosen_at_run_time_is_held_to_the_same_grammar() {
        let owned = ProviderName::owned("fake_0a1b".to_owned()).expect("a slug");
        assert_eq!(owned.as_str(), "fake_0a1b");
        assert_ne!(owned, ProviderName::new("fake"));
        assert_eq!(ProviderName::owned("Fake".to_owned()), None);
        assert_eq!(ProviderName::owned(String::new()), None);
        assert_eq!(ProviderName::owned("a".repeat(33)), None);
    }
}
