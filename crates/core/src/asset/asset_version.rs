//! [`AssetVersion`] — the version a content-addressed asset is linked by.

#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
use core::fmt;
#[cfg(feature = "std")]
use std::sync::Arc;

const LENGTH: usize = 43;

/// The version of an asset's bytes in `?v=`: exactly 43 characters of unpadded base64url.
///
/// That alphabet needs no escaping in an attribute or a URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetVersion(Arc<str>);

impl AssetVersion {
    /// Reads `text` as a version, or `None` when it is not exactly 43 base64url characters.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let canonical = text.len() == LENGTH
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        canonical.then(|| Self(Arc::from(text)))
    }

    /// The version as written in a URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA";

    #[test]
    fn accepts_exactly_43_base64url_characters() {
        let version = AssetVersion::parse(DIGEST).expect("a 43-character base64url digest");
        assert_eq!(version.as_str(), DIGEST);
        assert_eq!(version.to_string(), DIGEST);
        assert!(AssetVersion::parse("A-_9".repeat(10).as_str()).is_none());
        assert!(AssetVersion::parse(&format!("{DIGEST}A")).is_none());
        assert!(AssetVersion::parse("").is_none());
    }

    #[test]
    fn refuses_a_quote_an_ampersand_padding_and_standard_base64() {
        for bad in ['"', '&', '=', '+', '/', '<', ' '] {
            let text = format!("{}{bad}", &DIGEST[..42]);
            assert!(AssetVersion::parse(&text).is_none(), "{bad:?} was accepted");
        }
    }
}
