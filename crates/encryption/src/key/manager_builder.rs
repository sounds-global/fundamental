use std::collections::HashMap;

use subtle::ConstantTimeEq;

use crate::{EncryptionAlgorithm, KeyEntry, KeyError, KeyManager, KeyResult, KeySource};

/// Builds a `KeyManager`.
#[derive(Debug, Default)]
pub struct KeyManagerBuilder {
    keys: HashMap<String, KeyEntry>,
    default_algorithm: EncryptionAlgorithm,
    purpose_conflicts: Vec<String>,
}

impl KeyManagerBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the default encryption algorithm.
    pub fn default_algorithm(mut self, algorithm: EncryptionAlgorithm) -> Self {
        self.default_algorithm = algorithm;
        self
    }

    /// Adds a local key from raw bytes.
    ///
    /// For development and tests; production uses a KMS provider.
    ///
    /// # Panics
    ///
    /// Panics if the key is not exactly 32 bytes (256 bits).
    pub fn with_local_key(mut self, key_id: &str, key_bytes: &[u8]) -> Self {
        self.add_key_version(key_id, 1, key_bytes, self.default_algorithm, "");
        self
    }

    /// Adds a local key from a base64-encoded string.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 string is invalid or the decoded
    /// key is not exactly 32 bytes.
    pub fn with_local_key_base64(mut self, key_id: &str, key_base64: &str) -> KeyResult<Self> {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let key_bytes = STANDARD
            .decode(key_base64)
            .map_err(|e| KeyError::InvalidConfig(format!("invalid base64: {e}")))?;

        if key_bytes.len() != 32 {
            return Err(KeyError::InvalidConfig(format!(
                "key must be 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        self.add_key_version(key_id, 1, &key_bytes, self.default_algorithm, "");
        Ok(self)
    }

    /// Adds a key version declared for `purpose`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub(crate) fn add_key_version(
        &mut self,
        key_id: &str,
        version: u32,
        key_bytes: &[u8],
        algorithm: EncryptionAlgorithm,
        purpose: &str,
    ) {
        assert_eq!(key_bytes.len(), 32, "key must be 32 bytes (256 bits)");

        let entry = self
            .keys
            .entry(key_id.to_string())
            .or_insert_with(|| KeyEntry::new(key_id));
        if entry.versions.is_empty() {
            entry.purpose = purpose.to_owned();
        } else if entry.purpose != purpose {
            self.purpose_conflicts.push(format!(
                "key '{key_id}' is declared for purpose `{}` and for `{purpose}`",
                entry.purpose
            ));
        }

        entry.add_version(version, key_bytes.to_vec(), algorithm);
    }

    /// Adds a key version with a specific algorithm.
    pub fn with_key_version(
        mut self,
        key_id: &str,
        version: u32,
        key_bytes: &[u8],
        algorithm: EncryptionAlgorithm,
    ) -> Self {
        self.add_key_version(key_id, version, key_bytes, algorithm, "");
        self
    }

    /// Builds the `KeyManager`.
    ///
    /// # Errors
    ///
    /// [`KeyError::InvalidConfig`] when no key is configured, or when two key versions carry
    /// identical material.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn build(self) -> KeyResult<KeyManager> {
        if self.keys.is_empty() {
            return Err(KeyError::InvalidConfig(
                "at least one key must be configured".to_string(),
            ));
        }

        let mut held: Vec<(&str, u32, &[u8])> =
            self.keys
                .values()
                .flat_map(|entry| {
                    entry.versions.values().map(move |version| {
                        (entry.id.as_str(), version.version, &version.material[..])
                    })
                })
                .collect();
        held.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let mut problems = self.purpose_conflicts.clone();
        for (index, (first_id, first_version, first)) in held.iter().enumerate() {
            for (second_id, second_version, second) in held.iter().skip(index + 1) {
                if bool::from(first.ct_eq(second)) {
                    problems.push(format!(
                        "key '{first_id}' version {first_version} and key '{second_id}' version {second_version} carry identical material"
                    ));
                }
            }
        }
        if !problems.is_empty() {
            return Err(KeyError::InvalidConfig(problems.join("; ")));
        }

        Ok(KeyManager {
            keys: self.keys,
            default_algorithm: self.default_algorithm,
            fingerprints_declared: false,
        })
    }

    /// Builds a `KeyManager` from a `KeySource`.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_source<S: KeySource>(source: &S) -> KeyResult<KeyManager> {
        let configs = source.load_keys().await?;
        // Declared only when every version was.
        let declared =
            !configs.is_empty() && configs.iter().all(|config| config.fingerprint_declared);
        let mut builder = Self::new();

        for config in configs {
            builder.add_key_version(
                &config.id,
                config.version,
                &config.material,
                config.algorithm,
                &config.purpose,
            );
        }

        let mut manager = builder.build()?;
        manager.fingerprints_declared = declared;
        Ok(manager)
    }
}
