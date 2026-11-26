use serde::Deserialize;

/// The `Referrer-Policy` a surface sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferrerPolicy {
    /// The full URL to this origin, only the origin to another, nothing over a downgrade.
    #[default]
    StrictOriginWhenCrossOrigin,
    /// Nothing at all.
    NoReferrer,
}

impl ReferrerPolicy {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            Self::NoReferrer => "no-referrer",
        }
    }
}
