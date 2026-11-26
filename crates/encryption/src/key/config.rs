use crate::EncryptionAlgorithm;

/// Configuration for a single key.
#[derive(Debug, Clone)]
pub struct KeyConfig {
    /// The key identifier.
    pub id: String,
    /// The key material, 32 bytes for AES-256.
    pub material: Vec<u8>,
    /// The key version.
    pub version: u32,
    /// The encryption algorithm.
    pub algorithm: EncryptionAlgorithm,
    /// The purpose the service declared for this key, bound into every value sealed under it.
    pub purpose: String,
    /// Whether the material matched a fingerprint declared for this version when it loaded.
    pub fingerprint_declared: bool,
}

impl KeyConfig {
    /// Creates a new key configuration.
    pub fn new(id: impl Into<String>, material: Vec<u8>) -> Self {
        Self {
            id: id.into(),
            material,
            version: 1,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            purpose: String::new(),
            fingerprint_declared: false,
        }
    }

    /// Sets the key version.
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Sets the algorithm.
    pub fn with_algorithm(mut self, algorithm: EncryptionAlgorithm) -> Self {
        self.algorithm = algorithm;
        self
    }
}
