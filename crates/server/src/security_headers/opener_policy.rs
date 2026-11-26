/// The `Cross-Origin-Opener-Policy` a surface sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenerPolicy {
    /// A cross-origin window that opens this one, or that this one opens, gets no handle to it.
    #[default]
    SameOrigin,
    /// As [`SameOrigin`](Self::SameOrigin), except that a popup this surface opens keeps its handle.
    SameOriginAllowPopups,
}

impl OpenerPolicy {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::SameOrigin => "same-origin",
            Self::SameOriginAllowPopups => "same-origin-allow-popups",
        }
    }
}
