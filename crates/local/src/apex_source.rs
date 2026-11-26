use std::collections::BTreeMap;

use fundamental_core::address::TestApex;

use crate::ApexSet;

/// Where a project's apexes are declared: by its brands, or in `[local] apexes` — never both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApexSource {
    /// Each brand's `[domains] local`.
    Brands {
        /// The apex of each brand, by brand code.
        by_brand: BTreeMap<String, TestApex>,
        /// Every brand's apex.
        apexes: ApexSet,
    },
    /// `[local] apexes` in the manifest.
    Declared(ApexSet),
}

impl ApexSource {
    /// Every apex the source declares.
    #[must_use]
    pub const fn apexes(&self) -> &ApexSet {
        match self {
            Self::Brands { apexes, .. } | Self::Declared(apexes) => apexes,
        }
    }

    /// The apex of brand `code`, or `None` when the project has no such brand or no brands.
    #[must_use]
    pub fn brand(&self, code: &str) -> Option<&TestApex> {
        match self {
            Self::Brands { by_brand, .. } => by_brand.get(code),
            Self::Declared(_) => None,
        }
    }
}
