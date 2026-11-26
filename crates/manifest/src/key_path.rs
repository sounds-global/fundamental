//! A key in the manifest, as a path of segments.

use std::fmt;
use std::str::FromStr;

use toml_edit::Key;

use crate::ManifestError;

/// A key in the manifest — `server.port`, `custom.appendix_a."privacy.backup_days"` — as the
/// path of segments it names.
///
/// Parsed as TOML parses a dotted key, so a quoted segment that holds a dot stays one segment.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyPath(Vec<String>);

impl KeyPath {
    /// The path of these segments.
    #[must_use]
    pub fn from_segments<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(segments.into_iter().map(Into::into).collect())
    }

    /// The segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// Whether this is the path with no segments, the whole manifest.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `prefix` is this path or one of its ancestors.
    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// This path with `segment` appended.
    #[must_use]
    pub fn child(&self, segment: &str) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.to_owned());
        Self(segments)
    }
}

impl FromStr for KeyPath {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim().is_empty() {
            return Err(ManifestError::BadKey {
                key: text.to_owned(),
                cause: "a key names at least one segment".to_owned(),
            });
        }
        let keys = Key::parse(text).map_err(|error| ManifestError::BadKey {
            key: text.to_owned(),
            cause: error.message().to_owned(),
        })?;
        Ok(Self(keys.iter().map(|key| key.get().to_owned()).collect()))
    }
}

impl fmt::Display for KeyPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(".")?;
            }
            write!(formatter, "{}", Key::new(segment.as_str()).display_repr())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_segment_keeps_its_dot() -> Result<(), ManifestError> {
        let key: KeyPath = r#"custom.appendix_a."privacy.backup_days""#.parse()?;
        assert_eq!(
            key.segments(),
            ["custom", "appendix_a", "privacy.backup_days"]
        );
        assert_eq!(
            key.to_string(),
            r#"custom.appendix_a."privacy.backup_days""#
        );
        Ok(())
    }

    #[test]
    fn a_path_starts_with_itself_and_its_ancestors_only() -> Result<(), ManifestError> {
        let key: KeyPath = "server.tls".parse()?;
        assert!(key.starts_with(&"server".parse()?));
        assert!(key.starts_with(&key));
        assert!(!key.starts_with(&"server.tls.cert".parse()?));
        assert!(!key.starts_with(&"boa".parse()?));
        Ok(())
    }

    #[test]
    fn refuses_what_is_not_a_key() {
        for text in ["", "  ", "a..b", "a.", "[a]"] {
            let refused = text.parse::<KeyPath>();
            assert!(
                matches!(refused, Err(ManifestError::BadKey { .. })),
                "{text:?}: {refused:?}"
            );
        }
    }
}
