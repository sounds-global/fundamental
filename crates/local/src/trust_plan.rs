use rustls_pki_types::CertificateDer;

use crate::{ProjectName, RootFingerprint, TrustStore};

/// A root judged and ready to install, built before anything privileged runs: the DER read
/// once, what it permits, and every store it will go into.
#[derive(Debug, Clone)]
pub struct TrustPlan {
    pub(crate) project: ProjectName,
    pub(crate) der: CertificateDer<'static>,
    pub(crate) fingerprint: RootFingerprint,
    pub(crate) permitted: Vec<String>,
    pub(crate) stores: Vec<TrustStore>,
    pub(crate) minted: bool,
}

impl TrustPlan {
    /// The root's fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> &RootFingerprint {
        &self.fingerprint
    }

    /// The names the root's constraint permits, read from its own bytes.
    #[must_use]
    pub fn permitted(&self) -> &[String] {
        &self.permitted
    }

    /// Every store the root will be installed in, as a report names it.
    #[must_use]
    pub fn stores(&self) -> Vec<String> {
        self.stores.iter().map(TrustStore::describe).collect()
    }

    /// Whether a new root was minted for this.
    #[must_use]
    pub const fn minted(&self) -> bool {
        self.minted
    }
}
