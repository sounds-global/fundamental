//! [`AppKey`] — the key an app is declared under, `[apps.<key>]`.

use std::fmt;

/// The key of one app in the manifest, `www` or `www_days`: a lower-case ASCII letter, then
/// letters and digits, with a single `_` or `-` allowed between two of them, at most 63 bytes.
/// Narrow because `__` separates segments of `FUNDAMENTAL_APPS__<KEY>__…` variables.
///
/// ```
/// use fundamental_manifest::AppKey;
///
/// assert_eq!(AppKey::parse("www_days").map(|key| key.to_string()).as_deref(), Some("www_days"));
/// assert!(AppKey::parse("www__days").is_none());
/// assert!(AppKey::parse("Www").is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppKey(String);

impl AppKey {
    /// Parses `text` as an app key, or `None` when it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::is_key(text).then(|| Self(text.to_owned()))
    }

    /// Whether `text` is an app key.
    #[must_use]
    pub fn is_key(text: &str) -> bool {
        let bytes = text.as_bytes();
        let separator = |byte: &u8| *byte == b'_' || *byte == b'-';
        (1..=63).contains(&bytes.len())
            && bytes.first().is_some_and(u8::is_ascii_lowercase)
            && bytes.last().is_some_and(|byte| !separator(byte))
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator(byte))
            && !bytes
                .windows(2)
                .any(|pair| separator(&pair[0]) && separator(&pair[1]))
    }

    /// The key as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AppKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::AppKey;

    #[test]
    fn an_app_or_an_instance_key_is_admitted() {
        for key in ["www", "www_days", "mijn", "layer5-witness", "a1", "a"] {
            assert!(AppKey::is_key(key), "{key}");
        }
    }

    #[test]
    fn an_app_key_that_could_split_a_variable_is_refused() {
        for key in [
            "",
            "www__days",
            "_www",
            "www_",
            "-www",
            "www-",
            "w_-w",
            "Www",
            "1www",
            "www.days",
            "www days",
            "a\nb",
            "a{b}",
            "a/b",
        ] {
            assert!(AppKey::parse(key).is_none(), "{key:?}");
        }
        assert!(AppKey::parse(&"a".repeat(64)).is_none());
        assert!(AppKey::parse(&"a".repeat(63)).is_some());
    }
}
