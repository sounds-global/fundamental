use crate::RootFingerprint;

/// What `trust` installed.
#[derive(Debug)]
pub struct TrustReport {
    /// The root that is now trusted.
    pub fingerprint: RootFingerprint,
    /// Each store it is now in.
    pub stores: Vec<String>,
}
