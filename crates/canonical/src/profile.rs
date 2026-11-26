/// Which checks a canonical reader runs beyond the canonical form itself.
///
/// NFC and unassigned-code-point checks depend on the compiled Unicode version, so only the registry runs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Profile {
    /// Version-independent checks only: what a verifier or wallet runs.
    #[default]
    Verifier,
    /// Adds NFC and unassigned-code-point checks against [`UnicodeData::REGISTRY`](crate::UnicodeData::REGISTRY).
    #[cfg(feature = "registry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "registry")))]
    Registry,
}
