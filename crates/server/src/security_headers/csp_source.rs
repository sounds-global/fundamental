use fundamental_core::address::WebOrigin;

/// A source an image or font may be loaded from beside the page's own origin: an origin or `data:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspSource {
    /// One https origin.
    Origin(WebOrigin),
    /// Inline `data:` URLs — an embedded icon or a font subset.
    Data,
}

impl CspSource {
    pub(crate) fn expression(&self) -> &str {
        match self {
            Self::Origin(origin) => origin.as_str(),
            Self::Data => "data:",
        }
    }
}
