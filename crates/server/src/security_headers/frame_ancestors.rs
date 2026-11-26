use fundamental_core::address::WebOrigin;

/// Which pages may embed this surface in a frame (`frame-ancestors`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FrameAncestors {
    /// No page may frame this one; also sends `X-Frame-Options: DENY`.
    #[default]
    None,
    /// Only pages of this surface's own origin. Also sends `X-Frame-Options: SAMEORIGIN`.
    SelfOnly,
    /// Only these origins; no `X-Frame-Options`, which cannot express a list.
    Origins(Vec<WebOrigin>),
}

impl FrameAncestors {
    pub(crate) fn directive(&self) -> String {
        match self {
            Self::None => "frame-ancestors 'none'".to_owned(),
            Self::SelfOnly => "frame-ancestors 'self'".to_owned(),
            Self::Origins(origins) if origins.is_empty() => "frame-ancestors 'none'".to_owned(),
            Self::Origins(origins) => {
                let mut directive = String::from("frame-ancestors");
                for origin in origins {
                    directive.push(' ');
                    directive.push_str(origin.as_str());
                }
                directive
            }
        }
    }

    pub(crate) fn x_frame_options(&self) -> Option<&'static str> {
        match self {
            Self::None => Some("DENY"),
            Self::Origins(origins) if origins.is_empty() => Some("DENY"),
            Self::SelfOnly => Some("SAMEORIGIN"),
            Self::Origins(_) => None,
        }
    }
}
