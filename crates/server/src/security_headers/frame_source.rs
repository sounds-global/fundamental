use fundamental_core::address::WebOrigin;

/// An origin whose pages this surface may show in an `<iframe>` (`frame-src`), the one place a wildcard is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameSource {
    /// Exactly one origin.
    Origin(WebOrigin),
    /// Every https host directly or indirectly under `origin`'s host: `https://*.<host>`.
    SubdomainsOf(WebOrigin),
}

impl FrameSource {
    pub(crate) fn expression(&self) -> String {
        match self {
            Self::Origin(origin) => origin.as_str().to_owned(),
            Self::SubdomainsOf(origin) => format!("https://*.{}", origin.host()),
        }
    }
}
