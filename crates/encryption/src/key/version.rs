use zeroize::Zeroizing;

use crate::EncryptionAlgorithm;

#[derive(Clone)]
pub(crate) struct KeyVersion {
    pub(crate) version: u32,
    pub(crate) material: Zeroizing<Vec<u8>>,
    pub(crate) algorithm: EncryptionAlgorithm,
}

impl std::fmt::Debug for KeyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyVersion")
            .field("version", &self.version)
            .field("algorithm", &self.algorithm)
            .field("material", &"[REDACTED]")
            .finish()
    }
}
