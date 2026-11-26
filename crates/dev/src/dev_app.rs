//! [`DevApp`]: one application the dev loop builds, runs, and restarts.

use std::collections::BTreeMap;

/// An app under supervision: which cargo target produces it, and what environment it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevApp {
    /// How the app is named to a person: the log prefix and every report.
    pub name: String,
    /// The cargo package to build (`-p`).
    pub package: String,
    /// The binary target within that package (`--bin`).
    pub bin: String,
    /// Cargo features to enable on this package.
    pub features: Vec<String>,
    /// Environment applied on top of the inherited one when the app is launched.
    pub env: BTreeMap<String, String>,
}

impl DevApp {
    /// An app whose name, package, and binary are all the same word.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            package: name.clone(),
            bin: name.clone(),
            name,
            features: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    /// Enable a cargo feature on this app's package.
    #[must_use]
    pub fn feature(mut self, feature: impl Into<String>) -> Self {
        self.features.push(feature.into());
        self
    }

    /// Build it from a different cargo package than its name.
    #[must_use]
    pub fn package(mut self, package: impl Into<String>) -> Self {
        self.package = package.into();
        self
    }

    /// Run a binary target whose name differs from the app's.
    #[must_use]
    pub fn bin(mut self, bin: impl Into<String>) -> Self {
        self.bin = bin.into();
        self
    }

    /// Set one environment entry for the launched process.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set many environment entries at once.
    #[must_use]
    pub fn envs(mut self, entries: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env.extend(entries);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_app_names_its_own_package_and_binary() {
        let app = DevApp::new("layer0");
        assert_eq!(app.name, "layer0");
        assert_eq!(app.package, "layer0");
        assert_eq!(app.bin, "layer0");
        assert!(app.env.is_empty());
    }

    #[test]
    fn the_package_and_binary_can_differ_from_the_name() {
        let app = DevApp::new("layer5_witness").bin("witness");
        assert_eq!(app.package, "layer5_witness");
        assert_eq!(app.bin, "witness");
    }

    #[test]
    fn environment_entries_accumulate() {
        let app = DevApp::new("layer2")
            .env("ACME_LAYER2_ORIGIN", "https://layer2.acme.test")
            .envs([("ACME_BIND_HOST".to_string(), "0.0.0.0".to_string())]);
        assert_eq!(app.env.len(), 2);
        assert_eq!(
            app.env.get("ACME_LAYER2_ORIGIN").map(String::as_str),
            Some("https://layer2.acme.test")
        );
    }
}
