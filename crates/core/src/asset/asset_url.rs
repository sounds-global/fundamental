//! [`AssetUrl`] — the address a page links an asset by.

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
use core::fmt;

use crate::asset::AssetVersion;

/// The address a page writes for an asset: `path?v=<version>`, or the bare path when unversioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetUrl {
    path: Box<str>,
    version: Option<AssetVersion>,
}

impl AssetUrl {
    /// The address of `path` at `version`, or the bare path when there is none.
    #[must_use]
    pub fn new(path: &str, version: Option<AssetVersion>) -> Self {
        Self {
            path: Box::from(path),
            version,
        }
    }

    /// The asset's path, without the version.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The version this address names, if it names one.
    #[must_use]
    pub fn version(&self) -> Option<&AssetVersion> {
        self.version.as_ref()
    }
}

impl fmt::Display for AssetUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(version) => write!(f, "{}?v={version}", self.path),
            None => f.write_str(&self.path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_path_and_its_version() {
        let version = AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA")
            .expect("a valid version");
        let url = AssetUrl::new("/_fundamental/app.css", Some(version));
        assert_eq!(
            url.to_string(),
            "/_fundamental/app.css?v=q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA"
        );
        assert_eq!(url.path(), "/_fundamental/app.css");
    }

    #[test]
    fn an_unversioned_asset_is_its_bare_path() {
        let url = AssetUrl::new("/theme.js", None);
        assert_eq!(url.to_string(), "/theme.js");
        assert!(url.version().is_none());
    }
}
