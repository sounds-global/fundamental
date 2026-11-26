//! [`AssetAddresses`] — every versioned asset a server holds, by path.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, collections::BTreeMap};
#[cfg(feature = "std")]
use std::collections::BTreeMap;

use crate::asset::{AssetUrl, AssetVersion};

/// The version of every asset a server holds in memory, by the path it serves it at.
///
/// A path it does not hold is answered with the bare path, left to revalidate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetAddresses(BTreeMap<Box<str>, AssetVersion>);

impl AssetAddresses {
    /// Records that the asset at `path` is served at `version`, replacing any earlier one.
    pub fn insert(&mut self, path: &str, version: AssetVersion) {
        self.0.insert(Box::from(path), version);
    }

    /// Forgets the asset at `path`.
    pub fn remove(&mut self, path: &str) {
        self.0.remove(path);
    }

    /// The version the asset at `path` is served at, if it is versioned.
    #[must_use]
    pub fn version(&self, path: &str) -> Option<&AssetVersion> {
        self.0.get(path)
    }

    /// The address a page links the asset at `path` by.
    #[must_use]
    pub fn url(&self, path: &str) -> AssetUrl {
        AssetUrl::new(path, self.version(path).cloned())
    }

    /// Whether no asset is versioned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version() -> AssetVersion {
        AssetVersion::parse("q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA").expect("valid")
    }

    #[test]
    fn a_held_path_is_linked_by_its_version() {
        let mut addresses = AssetAddresses::default();
        addresses.insert("/_fundamental/app.css", version());
        assert_eq!(
            addresses.url("/_fundamental/app.css").version(),
            Some(&version())
        );
    }

    #[test]
    fn an_unknown_or_removed_path_is_its_plain_self() {
        let mut addresses = AssetAddresses::default();
        assert_eq!(addresses.url("/theme.js").to_string(), "/theme.js");
        addresses.insert("/theme.js", version());
        addresses.remove("/theme.js");
        assert_eq!(addresses.url("/theme.js").to_string(), "/theme.js");
        assert!(addresses.is_empty());
    }
}
