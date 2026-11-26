#[cfg(debug_assertions)]
use crate::{EncryptionAlgorithm, KeyConfig};
#[cfg(debug_assertions)]
use crate::{KeyResult, KeySource};

/// A development-only key source holding keys in memory.
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct LocalKeySource {
    keys: Vec<KeyConfig>,
}

#[cfg(debug_assertions)]
impl LocalKeySource {
    /// Creates a new local key source.
    pub fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Adds a key.
    pub fn with_key(mut self, id: &str, material: Vec<u8>) -> Self {
        self.keys.push(KeyConfig {
            id: id.to_string(),
            material,
            version: 1,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            purpose: String::new(),
            fingerprint_declared: false,
        });
        self
    }

    /// Generates a random key for development.
    ///
    /// Not persisted; different on each run.
    #[expect(clippy::missing_panics_doc, reason = "readability baseline")]
    pub fn with_random_key(mut self, id: &str) -> Self {
        use ring::rand::{SecureRandom, SystemRandom};

        let rng = SystemRandom::new();
        let mut material = vec![0u8; 32];
        rng.fill(&mut material)
            .expect("failed to generate random key");

        self.keys.push(KeyConfig {
            id: id.to_string(),
            material,
            version: 1,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            purpose: String::new(),
            fingerprint_declared: false,
        });
        self
    }
}

#[cfg(debug_assertions)]
impl Default for LocalKeySource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(debug_assertions)]
#[async_trait::async_trait]
impl KeySource for LocalKeySource {
    async fn load_keys(&self) -> KeyResult<Vec<KeyConfig>> {
        Ok(self.keys.clone())
    }
}
