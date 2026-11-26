//! [`PackageProfile`] — what a package was built for.

use serde::{Deserialize, Serialize};

use crate::PackageBounds;

/// What a package was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageProfile {
    /// A release build with the feedback layer, served on a preview origin.
    Preview,
    /// A release build without the feedback layer, served at the app's own origin.
    Production,
}

impl PackageProfile {
    /// The profile's name, as `package.json` and a package's file name spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Production => "production",
        }
    }

    /// The most a package of this profile may hold, written and unpacked.
    #[must_use]
    pub const fn bounds(self) -> PackageBounds {
        match self {
            Self::Preview => PackageBounds::PREVIEW,
            Self::Production => PackageBounds::PRODUCTION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_production_package_says_production_in_its_manifest() {
        let manifest =
            crate::PackageManifest::new("www", PackageProfile::Production, "457b125", vec![])
                .expect("a manifest");
        let json = String::from_utf8(manifest.to_json()).expect("utf-8");

        assert!(json.contains("\"profile\": \"production\""), "{json}");
        assert_eq!(
            crate::PackageManifest::parse(json.as_bytes()).expect("read back"),
            manifest
        );
    }

    #[test]
    fn a_production_package_fits_wherever_its_preview_fits() {
        let preview = PackageProfile::Preview.bounds();
        let production = PackageProfile::Production.bounds();

        assert!(production.entries >= preview.entries);
        assert!(production.unpacked >= preview.unpacked);
        assert!(production.packed >= preview.packed);
    }
}
