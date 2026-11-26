use std::collections::HashMap;

use fundamental_config::settings::{EncryptionSettings, KeyAlgorithm, KeyBackend};
use fundamental_core::env::EnvVars;
use fundamental_secrets::VaultClient;
use serde::{Serialize, de::DeserializeOwned};
use subtle::ConstantTimeEq;
use tracing::field::Empty;
use tracing::{Span, debug, info, instrument};
use zeroize::Zeroizing;

use crate::fns::length_prefixed;
use crate::{
    ConfiguredKey, ConfiguredKeySource, DEFAULT_TRANSIT_MOUNT, Encrypted, EncryptedBytes,
    EncryptedKeyConfig, EncryptedString, EncryptionAlgorithm, EncryptionMetadata, Encryptor,
    EnvKeySource, EnvelopeKeySource, KeyEntry, KeyError, KeyManagerBuilder, KeyResult,
    ROW_BOUND_FORMAT, RowBinding, SharedKeyManager, VaultKmsProvider,
};

/// Holds the encryption keys with their versions and encrypts and decrypts with them.
///
/// # Examples
///
/// ```no_run
/// use fundamental_encryption::KeyManager;
///
/// # fn main() -> Result<(), fundamental_encryption::KeyError> {
/// // Create with local keys (development), then attach to a connection registry
/// // with `.with_key_manager(..)` so `Secret` model fields encrypt at rest.
/// let key_bytes = [0u8; 32];
/// let manager = KeyManager::builder()
///     .with_local_key("default", &key_bytes)
///     .build()?;
///
/// assert_eq!(manager.current_version("default")?, 1);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct KeyManager {
    pub(crate) keys: HashMap<String, KeyEntry>,
    /// The default algorithm to use for new encryptions.
    pub default_algorithm: EncryptionAlgorithm,
    pub(crate) fingerprints_declared: bool,
}

impl KeyManager {
    /// Creates a new `KeyManager` builder.
    pub fn builder() -> KeyManagerBuilder {
        KeyManagerBuilder::new()
    }

    /// Builds a shared manager from `{prefix}_{K}` environment variables, each a base64 32-byte
    /// key, versioned by `{prefix}_{K}_VERSION`.
    #[instrument(skip_all, fields(backend = "env", keys = Empty))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_env_keys<I, S>(
        key_ids: I,
        prefix: impl Into<String>,
    ) -> KeyResult<SharedKeyManager>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let ids: Vec<String> = key_ids.into_iter().map(Into::into).collect();
        Span::current().record("keys", ids.len());
        debug!(keys = ids.len(), "loading encryption keys from env source");
        let source = EnvKeySource::with_prefix(prefix, ids);
        let manager = KeyManagerBuilder::from_source(&source).await?;
        info!(keys = manager.keys.len(), "key manager ready (env backend)");
        Ok(shared_with_fingerprints_logged(manager))
    }

    /// Builds a shared manager from [`EncryptionSettings`], with the key source its backend names.
    ///
    /// [`KeyBackend::Vault`] requires `vault`, returning [`KeyError::Kms`] without it.
    #[instrument(skip_all, fields(backend = ?settings.backend))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn from_settings(
        settings: &EncryptionSettings,
        vault: Option<&VaultClient>,
    ) -> KeyResult<SharedKeyManager> {
        Self::from_settings_reading_env_from(settings, vault, EnvVars::process()).await
    }

    /// The same, reading [`KeyBackend::Env`] material from `vars`.
    ///
    /// Lets the env backend be tested without mutating the process environment.
    ///
    /// # Errors
    /// The same as [`from_settings`](Self::from_settings).
    #[instrument(skip_all, fields(backend = ?settings.backend))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn from_settings_reading_env_from(
        settings: &EncryptionSettings,
        vault: Option<&VaultClient>,
        vars: EnvVars,
    ) -> KeyResult<SharedKeyManager> {
        match settings.backend {
            KeyBackend::Env => {
                debug!("selected env key backend");
                let source = EnvKeySource::with_prefix(
                    settings.env_prefix.clone(),
                    settings.key_ids.clone(),
                )
                .with_vars(vars);
                let manager = KeyManagerBuilder::from_source(&source).await?;
                info!(keys = manager.keys.len(), "key manager ready (env backend)");
                Ok(shared_with_fingerprints_logged(manager))
            }
            KeyBackend::Config => {
                debug!("selected config key backend");

                // An empty list is a misconfiguration, not "no encryption".
                if settings.config_keys.is_empty() {
                    return Err(KeyError::InvalidConfig(
                        "config key backend selected but no `config_keys` configured".to_string(),
                    ));
                }

                // A purpose comes only from the service, never from configuration.
                let mut problems = Vec::new();
                for key in &settings.config_keys {
                    if !settings.purposes.contains_key(&key.id) {
                        problems.push(format!(
                            "key '{}' has no purpose: the service declares it with `EncryptionSettings::with_purpose`",
                            key.id
                        ));
                    }
                }
                for id in settings.purposes.keys() {
                    if !settings.config_keys.iter().any(|key| &key.id == id) {
                        problems.push(format!(
                            "the service expects key '{id}', but no `config_keys` entry configures it"
                        ));
                    }
                }
                if !problems.is_empty() {
                    return Err(KeyError::InvalidConfig(problems.join("; ")));
                }

                let keys = settings
                    .config_keys
                    .iter()
                    .map(|key| ConfiguredKey {
                        id: key.id.clone(),
                        purpose: settings.purposes.get(&key.id).cloned().unwrap_or_default(),
                        material: key.material.clone(),
                        fingerprint: key.fingerprint.clone(),
                        version: key.version,
                        algorithm: map_key_algorithm(key.algorithm),
                    })
                    .collect();
                let source = ConfiguredKeySource::new(keys);
                let manager = KeyManagerBuilder::from_source(&source).await?;
                info!(
                    keys = manager.keys.len(),
                    "key manager ready (config backend)"
                );
                Ok(shared_with_fingerprints_logged(manager))
            }
            KeyBackend::Vault => {
                use base64::{Engine, engine::general_purpose::STANDARD};

                debug!("selected vault key backend");
                let vault = vault.ok_or_else(|| {
                    KeyError::Kms(
                        "vault key backend selected but no vault client provided".to_string(),
                    )
                })?;

                if settings.keys.is_empty() {
                    return Err(KeyError::InvalidConfig(
                        "vault key backend selected but no `keys` configured".to_string(),
                    ));
                }

                let configs: Vec<EncryptedKeyConfig> = settings
                    .keys
                    .iter()
                    .map(|k| EncryptedKeyConfig {
                        id: k.id.clone(),
                        encrypted_dek: STANDARD.encode(k.encrypted_dek.as_bytes()),
                        kms_key_id: k.transit_key.clone(),
                        version: k.version,
                        algorithm: map_key_algorithm(k.algorithm),
                        fingerprint: k.fingerprint.clone(),
                    })
                    .collect();

                let provider = VaultKmsProvider::new(vault.clone(), DEFAULT_TRANSIT_MOUNT);
                let source = EnvelopeKeySource::new(Box::new(provider), configs);
                let manager = KeyManagerBuilder::from_source(&source).await?;
                info!(
                    keys = manager.keys.len(),
                    "key manager ready (vault backend)"
                );
                Ok(shared_with_fingerprints_logged(manager))
            }
        }
    }

    /// Returns the default encryption algorithm.
    pub fn default_algorithm(&self) -> EncryptionAlgorithm {
        self.default_algorithm
    }

    /// Returns the list of available key IDs.
    pub fn key_ids(&self) -> Vec<&str> {
        self.keys.keys().map(String::as_str).collect()
    }

    /// Returns the current version of a key.
    #[instrument(level = "debug", skip(self), fields(key_id))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn current_version(&self, key_id: &str) -> KeyResult<u32> {
        self.keys
            .get(key_id)
            .map(|k| k.current_version)
            .ok_or_else(|| KeyError::KeyNotFound(key_id.to_string()))
    }

    fn generate_iv(&self, algorithm: EncryptionAlgorithm) -> KeyResult<Vec<u8>> {
        use ring::rand::{SecureRandom, SystemRandom};

        let rng = SystemRandom::new();
        let len = match algorithm {
            EncryptionAlgorithm::Aes256Gcm => 12,        // 96 bits
            EncryptionAlgorithm::ChaCha20Poly1305 => 12, // 96 bits
        };

        let mut iv = vec![0u8; len];
        rng.fill(&mut iv)
            .map_err(|_| KeyError::Encryption("system RNG failed to generate IV".to_string()))?;
        Ok(iv)
    }

    #[instrument(level = "debug", skip(self, plaintext), fields(key_id, plaintext_len = plaintext.len()))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn encrypt_raw(
        &self,
        key_id: &str,
        plaintext: &[u8],
        row: &RowBinding<'_>,
    ) -> KeyResult<(Vec<u8>, EncryptionMetadata)> {
        let key_entry = self
            .keys
            .get(key_id)
            .ok_or_else(|| KeyError::KeyNotFound(key_id.to_string()))?;

        let key_version = key_entry
            .current()
            .ok_or_else(|| KeyError::KeyVersionNotFound {
                key_id: key_id.to_string(),
                version: key_entry.current_version,
            })?;

        debug!(
            version = key_version.version,
            algorithm = ?key_version.algorithm,
            "selected key version for encryption"
        );

        let iv = self.generate_iv(key_version.algorithm)?;
        let aad = row.associated_data(&key_entry.purpose);

        let ciphertext = match key_version.algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                self.encrypt_aes_gcm(&key_version.material, &iv, plaintext, &aad)?
            }
            EncryptionAlgorithm::ChaCha20Poly1305 => {
                self.encrypt_chacha20(&key_version.material, &iv, plaintext, &aad)?
            }
        };

        let metadata =
            EncryptionMetadata::new(key_id, key_version.version, key_version.algorithm, iv);

        Ok((ciphertext, metadata))
    }

    /// Decrypts raw bytes using the metadata.
    #[instrument(
        level = "debug",
        skip(self, ciphertext),
        fields(
            key_id = %metadata.key_id,
            key_version = metadata.key_version,
            ciphertext_len = ciphertext.len(),
            algorithm = ?metadata.algorithm,
        )
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn decrypt_raw(
        &self,
        ciphertext: &[u8],
        metadata: &EncryptionMetadata,
        row: &RowBinding<'_>,
    ) -> KeyResult<Vec<u8>> {
        // An envelope from before row binding was sealed to nothing, so it is refused.
        if metadata.format != ROW_BOUND_FORMAT {
            return Err(KeyError::Decryption(format!(
                "envelope format {} is not bound to a row; re-encrypt it",
                metadata.format
            )));
        }
        let key_entry = self
            .keys
            .get(&metadata.key_id)
            .ok_or_else(|| KeyError::KeyNotFound(metadata.key_id.clone()))?;

        let key_version = key_entry.version(metadata.key_version).ok_or_else(|| {
            KeyError::KeyVersionNotFound {
                key_id: metadata.key_id.clone(),
                version: metadata.key_version,
            }
        })?;
        debug!("resolved key version for decryption");
        let aad = row.associated_data(&key_entry.purpose);

        match metadata.algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                self.decrypt_aes_gcm(&key_version.material, &metadata.iv, ciphertext, &aad)
            }
            EncryptionAlgorithm::ChaCha20Poly1305 => {
                self.decrypt_chacha20(&key_version.material, &metadata.iv, ciphertext, &aad)
            }
        }
    }

    fn encrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        plaintext: &[u8],
        aad: &[u8],
    ) -> KeyResult<Vec<u8>> {
        use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

        let unbound_key = UnboundKey::new(&AES_256_GCM, key)
            .map_err(|e| KeyError::Encryption(format!("invalid key: {e}")))?;
        let key = LessSafeKey::new(unbound_key);

        let nonce = Nonce::try_assume_unique_for_key(iv)
            .map_err(|_| KeyError::Encryption("invalid nonce length".to_string()))?;

        let mut in_out = plaintext.to_vec();
        key.seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
            .map_err(|e| KeyError::Encryption(format!("encryption failed: {e}")))?;

        Ok(in_out)
    }

    fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> KeyResult<Vec<u8>> {
        use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

        let unbound_key = UnboundKey::new(&AES_256_GCM, key)
            .map_err(|e| KeyError::Decryption(format!("invalid key: {e}")))?;
        let key = LessSafeKey::new(unbound_key);

        let nonce = Nonce::try_assume_unique_for_key(iv)
            .map_err(|_| KeyError::Decryption("invalid nonce length".to_string()))?;

        let mut in_out = ciphertext.to_vec();
        let plaintext = key
            .open_in_place(nonce, Aad::from(aad), &mut in_out)
            .map_err(|e| KeyError::Decryption(format!("decryption failed: {e}")))?;

        Ok(plaintext.to_vec())
    }

    fn encrypt_chacha20(
        &self,
        key: &[u8],
        iv: &[u8],
        plaintext: &[u8],
        aad: &[u8],
    ) -> KeyResult<Vec<u8>> {
        use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

        let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key)
            .map_err(|e| KeyError::Encryption(format!("invalid key: {e}")))?;
        let key = LessSafeKey::new(unbound_key);

        let nonce = Nonce::try_assume_unique_for_key(iv)
            .map_err(|_| KeyError::Encryption("invalid nonce length".to_string()))?;

        let mut in_out = plaintext.to_vec();
        key.seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
            .map_err(|e| KeyError::Encryption(format!("encryption failed: {e}")))?;

        Ok(in_out)
    }

    fn decrypt_chacha20(
        &self,
        key: &[u8],
        iv: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> KeyResult<Vec<u8>> {
        use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

        let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key)
            .map_err(|e| KeyError::Decryption(format!("invalid key: {e}")))?;
        let key = LessSafeKey::new(unbound_key);

        let nonce = Nonce::try_assume_unique_for_key(iv)
            .map_err(|_| KeyError::Decryption("invalid nonce length".to_string()))?;

        let mut in_out = ciphertext.to_vec();
        let plaintext = key
            .open_in_place(nonce, Aad::from(aad), &mut in_out)
            .map_err(|e| KeyError::Decryption(format!("decryption failed: {e}")))?;

        Ok(plaintext.to_vec())
    }

    /// Encrypts a string into an envelope carrier.
    #[doc(hidden)]
    #[instrument(skip(self, plaintext), fields(key_id, plaintext_len = plaintext.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn encrypt_string(
        &self,
        plaintext: &str,
        key_id: &str,
        row: &RowBinding<'_>,
    ) -> KeyResult<EncryptedString> {
        let (ciphertext, metadata) = self.encrypt_raw(key_id, plaintext.as_bytes(), row)?;
        Ok(EncryptedString::from_encrypted(ciphertext, metadata))
    }

    /// Decrypts a string envelope carrier.
    #[doc(hidden)]
    #[instrument(
        skip(self, encrypted),
        fields(key_id = %encrypted.metadata().key_id, ciphertext_len = encrypted.ciphertext().len()),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn decrypt_string(
        &self,
        encrypted: &EncryptedString,
        row: &RowBinding<'_>,
    ) -> KeyResult<String> {
        let plaintext = self.decrypt_raw(encrypted.ciphertext(), encrypted.metadata(), row)?;
        String::from_utf8(plaintext)
            .map_err(|e| KeyError::Decryption(format!("invalid UTF-8: {e}")))
    }

    /// Encrypts bytes into an envelope carrier.
    #[doc(hidden)]
    #[instrument(skip(self, plaintext), fields(key_id, plaintext_len = plaintext.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn encrypt_bytes(
        &self,
        plaintext: &[u8],
        key_id: &str,
        row: &RowBinding<'_>,
    ) -> KeyResult<EncryptedBytes> {
        let (ciphertext, metadata) = self.encrypt_raw(key_id, plaintext, row)?;
        Ok(EncryptedBytes::from_encrypted(ciphertext, metadata))
    }

    /// Decrypts a bytes envelope carrier.
    #[doc(hidden)]
    #[instrument(
        skip(self, encrypted),
        fields(key_id = %encrypted.metadata().key_id, ciphertext_len = encrypted.ciphertext().len()),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn decrypt_bytes(
        &self,
        encrypted: &EncryptedBytes,
        row: &RowBinding<'_>,
    ) -> KeyResult<Vec<u8>> {
        self.decrypt_raw(encrypted.ciphertext(), encrypted.metadata(), row)
    }

    /// Serializes a value to JSON and encrypts it into a string envelope carrier.
    #[doc(hidden)]
    #[instrument(skip(self, value), fields(key_id, value_type = std::any::type_name::<T>()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn encrypt_json_to_string<T: Serialize>(
        &self,
        value: &T,
        key_id: &str,
        row: &RowBinding<'_>,
    ) -> KeyResult<EncryptedString> {
        let json =
            serde_json::to_string(value).map_err(|e| KeyError::Serialization(e.to_string()))?;
        self.encrypt_string(&json, key_id, row)
    }

    /// Decrypts a string envelope carrier and deserializes its JSON payload.
    #[doc(hidden)]
    #[instrument(
        skip(self, encrypted),
        fields(
            key_id = %encrypted.metadata().key_id,
            ciphertext_len = encrypted.ciphertext().len(),
            value_type = std::any::type_name::<T>(),
        ),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn decrypt_string_to_json<T: DeserializeOwned>(
        &self,
        encrypted: &EncryptedString,
        row: &RowBinding<'_>,
    ) -> KeyResult<T> {
        let json = self.decrypt_string(encrypted, row)?;
        serde_json::from_str(&json).map_err(|e| KeyError::Serialization(e.to_string()))
    }
}

impl KeyManager {
    /// A non-secret fingerprint of every held key version, ordered by key id and then version.
    ///
    /// The same material shows the same fingerprint as an at-rest key or as a keyring version.
    #[must_use]
    pub fn fingerprints(&self) -> Vec<(&str, u32, [u8; 32])> {
        let mut fingerprints: Vec<(&str, u32, [u8; 32])> = self
            .keys
            .iter()
            .flat_map(|(id, entry)| {
                entry.versions.values().map(move |version| {
                    (
                        id.as_str(),
                        version.version,
                        crate::material_fingerprint(&version.material),
                    )
                })
            })
            .collect();
        fingerprints.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        fingerprints
    }

    /// Whether every held key version matched a fingerprint its configuration declared, so a
    /// value that fails to authenticate is the value's fault rather than this process's key.
    #[must_use]
    pub fn fingerprints_declared(&self) -> bool {
        self.fingerprints_declared
    }

    /// The key id every outbox seal is made under.
    ///
    /// Its own key, so a seal is never made under the key the row's columns are sealed with.
    pub const OUTBOX_SEAL_KEY: &'static str = "outbox_seal";

    /// How many bytes a stored outbox seal holds: the key version, then the tag.
    pub const OUTBOX_SEAL_LEN: usize = 4 + 32;

    /// The seal a `#[model(outbox)]` row carries: the current `outbox_seal` key version as four
    /// big-endian bytes, then a keyed BLAKE3 tag over `table` and `parts` (the row's key and every
    /// sealed column; a `NULL` part is framed, not left out).
    ///
    /// # Errors
    /// [`KeyError::KeyNotFound`] when no `outbox_seal` key is configured, or
    /// [`KeyError::KeyVersionNotFound`] when its current version is absent.
    #[instrument(level = "debug", skip(self, parts), fields(table, parts = parts.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn seal_outbox_row(
        &self,
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<[u8; Self::OUTBOX_SEAL_LEN]> {
        self.seal_with(OutboxSealed::Row, table, parts)
    }

    /// Checks the seal a stored outbox row carries against the row it was read from.
    ///
    /// Constant-time, so a planted seal learns nothing from the refusal's timing.
    ///
    /// # Errors
    /// [`KeyError::Serialization`] when `stored` is not a seal at all;
    /// [`KeyError::KeyNotFound`] or [`KeyError::KeyVersionNotFound`] when the version it names is
    /// not held here, which another process may hold; [`KeyError::Decryption`] when the tag does
    /// not match, so the row was written, or edited, by something that does not hold the key.
    #[instrument(level = "debug", skip(self, stored, parts), fields(table, parts = parts.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn check_outbox_seal(
        &self,
        stored: &[u8],
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<()> {
        self.check_with(OutboxSealed::Row, stored, table, parts)
    }

    /// The seal over a set-aside row's key, instant and reason, under the `outbox_seal` key with
    /// its own label so it never checks as a row seal.
    ///
    /// # Errors
    /// The same as [`seal_outbox_row`](Self::seal_outbox_row).
    #[instrument(level = "debug", skip(self, parts), fields(table, parts = parts.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn seal_outbox_set_aside(
        &self,
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<[u8; Self::OUTBOX_SEAL_LEN]> {
        self.seal_with(OutboxSealed::SetAside, table, parts)
    }

    /// Checks the seal stored beside a set-aside row against the key, instant and reason it holds.
    ///
    /// # Errors
    /// The same as [`check_outbox_seal`](Self::check_outbox_seal).
    #[instrument(level = "debug", skip(self, stored, parts), fields(table, parts = parts.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn check_outbox_set_aside(
        &self,
        stored: &[u8],
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<()> {
        self.check_with(OutboxSealed::SetAside, stored, table, parts)
    }

    /// A keyed BLAKE3 tag over `label` and `parts` under a subkey of the current version of
    /// `key_id`, for finding or counting a value without storing it.
    ///
    /// Made with the current version only, so after a rotation the same parts tag differently.
    ///
    /// # Errors
    /// [`KeyError::KeyNotFound`] when `key_id` is not held, or [`KeyError::KeyVersionNotFound`]
    /// when its current version is absent.
    #[instrument(level = "debug", skip(self, parts), fields(key_id, label, parts = parts.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn tag(&self, key_id: &str, label: &str, parts: &[&[u8]]) -> KeyResult<[u8; 32]> {
        let entry = self
            .keys
            .get(key_id)
            .ok_or_else(|| KeyError::KeyNotFound(key_id.to_owned()))?;
        let current = entry
            .current()
            .ok_or_else(|| KeyError::KeyVersionNotFound {
                key_id: key_id.to_owned(),
                version: entry.current_version,
            })?;
        let subkey = Zeroizing::new(blake3::derive_key(
            "fundamental key manager tag 2026-10-08",
            &current.material,
        ));
        let framed = length_prefixed(
            [label.as_bytes(), entry.purpose.as_bytes()]
                .into_iter()
                .chain(parts.iter().copied())
                .collect::<Vec<&[u8]>>()
                .into_iter(),
        );
        Ok(*blake3::keyed_hash(&subkey, &framed).as_bytes())
    }

    fn seal_with(
        &self,
        sealed: OutboxSealed,
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<[u8; Self::OUTBOX_SEAL_LEN]> {
        let entry = self
            .keys
            .get(Self::OUTBOX_SEAL_KEY)
            .ok_or_else(|| KeyError::KeyNotFound(Self::OUTBOX_SEAL_KEY.to_owned()))?;
        let current = entry
            .current()
            .ok_or_else(|| KeyError::KeyVersionNotFound {
                key_id: Self::OUTBOX_SEAL_KEY.to_owned(),
                version: entry.current_version,
            })?;
        let tag = outbox_seal_tag(sealed, &current.material, &entry.purpose, table, parts);
        let mut seal = [0_u8; Self::OUTBOX_SEAL_LEN];
        seal[..4].copy_from_slice(&current.version.to_be_bytes());
        seal[4..].copy_from_slice(&tag);
        Ok(seal)
    }

    fn check_with(
        &self,
        sealed: OutboxSealed,
        stored: &[u8],
        table: &str,
        parts: &[(&str, Option<Vec<u8>>)],
    ) -> KeyResult<()> {
        let Some((version, tag)) = stored
            .split_first_chunk::<4>()
            .filter(|_| stored.len() == Self::OUTBOX_SEAL_LEN)
        else {
            return Err(KeyError::Serialization(format!(
                "an outbox seal is {} bytes, not {}",
                Self::OUTBOX_SEAL_LEN,
                stored.len()
            )));
        };
        let version = u32::from_be_bytes(*version);
        let entry = self
            .keys
            .get(Self::OUTBOX_SEAL_KEY)
            .ok_or_else(|| KeyError::KeyNotFound(Self::OUTBOX_SEAL_KEY.to_owned()))?;
        let held = entry
            .version(version)
            .ok_or_else(|| KeyError::KeyVersionNotFound {
                key_id: Self::OUTBOX_SEAL_KEY.to_owned(),
                version,
            })?;
        let expected = outbox_seal_tag(sealed, &held.material, &entry.purpose, table, parts);
        if bool::from(expected.as_slice().ct_eq(tag)) {
            Ok(())
        } else {
            Err(KeyError::Decryption(format!(
                "the outbox {} seal of a {table} row does not match what it seals",
                sealed.name()
            )))
        }
    }
}

/// What an outbox seal vouches for, each framed under its own label.
#[derive(Debug, Clone, Copy)]
enum OutboxSealed {
    /// A row the model wrote.
    Row,
    /// A claim's decision to set a row aside.
    SetAside,
}

impl OutboxSealed {
    const fn label(self) -> &'static [u8] {
        match self {
            Self::Row => b"fundamental outbox seal",
            Self::SetAside => b"fundamental outbox set-aside seal",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::SetAside => "set-aside",
        }
    }
}

const OUTBOX_SEAL_FORMAT: u8 = 1;

/// The keyed BLAKE3 tag over a row's table and its named parts, under a subkey of `material`;
/// each part carries a presence byte so `NULL` and empty never tag alike.
fn outbox_seal_tag(
    sealed: OutboxSealed,
    material: &[u8],
    purpose: &str,
    table: &str,
    parts: &[(&str, Option<Vec<u8>>)],
) -> [u8; 32] {
    const PRESENT: &[u8] = &[1];
    const ABSENT: &[u8] = &[0];
    let subkey = Zeroizing::new(blake3::derive_key(
        "fundamental outbox seal 2026-09-13 tag",
        material,
    ));
    let mut named: Vec<(&[u8], &[u8], &[u8])> = parts
        .iter()
        .map(|(name, bytes)| match bytes {
            Some(bytes) => (name.as_bytes(), PRESENT, bytes.as_slice()),
            None => (name.as_bytes(), ABSENT, &[][..]),
        })
        .collect();
    named.sort_unstable();
    let format = [OUTBOX_SEAL_FORMAT];
    let header: [&[u8]; 4] = [
        sealed.label(),
        &format,
        purpose.as_bytes(),
        table.as_bytes(),
    ];
    let framed: Vec<&[u8]> = header
        .into_iter()
        .chain(
            named
                .into_iter()
                .flat_map(|(name, presence, bytes)| [name, presence, bytes]),
        )
        .collect();
    *blake3::keyed_hash(&subkey, &length_prefixed(framed.into_iter())).as_bytes()
}

/// Shares `manager` after logging each key version's non-secret fingerprint.
fn shared_with_fingerprints_logged(manager: KeyManager) -> SharedKeyManager {
    for (key_id, version, fingerprint) in manager.fingerprints() {
        let fingerprint: String = fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        info!(key_id, version, %fingerprint, "encryption key loaded");
    }
    std::sync::Arc::new(manager)
}

fn map_key_algorithm(algorithm: KeyAlgorithm) -> EncryptionAlgorithm {
    match algorithm {
        KeyAlgorithm::Aes256Gcm => EncryptionAlgorithm::Aes256Gcm,
        KeyAlgorithm::ChaCha20Poly1305 => EncryptionAlgorithm::ChaCha20Poly1305,
    }
}

impl Encryptor for KeyManager {
    type Error = KeyError;

    #[instrument(level = "debug", skip(self, plaintext), fields(key_id, plaintext_len = plaintext.len()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn encrypt_bytes(
        &self,
        key_id: &str,
        plaintext: &[u8],
        row: &RowBinding<'_>,
    ) -> Result<(Vec<u8>, EncryptionMetadata), Self::Error> {
        self.encrypt_raw(key_id, plaintext, row)
    }

    #[instrument(
        level = "debug",
        skip(self, ciphertext),
        fields(key_id = %metadata.key_id, ciphertext_len = ciphertext.len()),
        err(Debug)
    )]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn decrypt_bytes(
        &self,
        ciphertext: &[u8],
        metadata: &EncryptionMetadata,
        row: &RowBinding<'_>,
    ) -> Result<Vec<u8>, Self::Error> {
        self.decrypt_raw(ciphertext, metadata, row)
    }
}

#[cfg(test)]
mod from_settings_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use fundamental_config::settings::ConfigKeyRef;
    use fundamental_core::secrets::Secret;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn secrets_never_logged_and_decrypt_failure_names_the_key_id() {
        let key = [0u8; 32];
        let manager = KeyManager::builder()
            .with_local_key("canary-key", &key)
            .build()
            .expect("manager builds");

        let secret = "super-secret-canary-plaintext";
        let parts = vec![("id", Some(b"\x01row".to_vec()))];
        let row = RowBinding::new("canaries", "secret", &parts);
        let encrypted = manager
            .encrypt_string(secret, "canary-key", &row)
            .expect("encrypt succeeds");
        assert_eq!(
            manager
                .decrypt_string(&encrypted, &row)
                .expect("round-trip decrypts"),
            secret
        );

        // The failure names the key id.
        let other = KeyManager::builder()
            .with_local_key("other-key", &key)
            .build()
            .expect("manager builds");
        let err = other
            .decrypt_string(&encrypted, &row)
            .expect_err("decryption without the key must fail");
        assert!(matches!(err, KeyError::KeyNotFound(_)));
        assert!(
            logs_contain("canary-key"),
            "decrypt failure must record the key id"
        );

        assert!(
            !logs_contain("super-secret-canary-plaintext"),
            "plaintext must never appear in any log event"
        );
    }

    #[tokio::test]
    async fn env_backend_loads_keys() {
        let key = STANDARD.encode([7u8; 32]);
        // Handed in as a value: setting the process environment races other tests.
        let vars = EnvVars::fixed([("TEST_ENC_FROM_SETTINGS_DEFAULT", key.as_str())]);

        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Env,
            key_ids: vec!["default".to_string()],
            env_prefix: "TEST_ENC_FROM_SETTINGS".to_string(),
            ..Default::default()
        };

        let manager = KeyManager::from_settings_reading_env_from(&settings, None, vars)
            .await
            .unwrap();
        assert!(manager.key_ids().contains(&"default"));
    }

    #[tokio::test]
    async fn vault_backend_without_client_errors() {
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Vault,
            key_ids: Vec::new(),
            env_prefix: "UNUSED".to_string(),
            ..Default::default()
        };

        let err = KeyManager::from_settings(&settings, None)
            .await
            .expect_err("vault backend without a client must error");
        assert!(matches!(err, KeyError::Kms(_)));
    }

    /// The key reaches the manager without ever being an environment variable.
    #[tokio::test]
    async fn config_backend_loads_keys_from_configuration() {
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Config,
            config_keys: vec![ConfigKeyRef {
                id: "pii".to_string(),
                material: Secret::new(format!("test.at-rest.v1:1:{}", "09".repeat(32))),
                version: 1,
                algorithm: KeyAlgorithm::Aes256Gcm,
                fingerprint: None,
            }],
            ..Default::default()
        }
        .with_purpose("pii", "test.at-rest.v1");

        let manager = KeyManager::from_settings(&settings, None)
            .await
            .expect("the configured key loads");

        assert!(manager.key_ids().contains(&"pii"));
    }

    #[tokio::test]
    async fn config_backend_refuses_a_key_labelled_for_another_purpose() {
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Config,
            config_keys: vec![ConfigKeyRef {
                id: "pii".to_string(),
                material: Secret::new(format!("test.layer-one.at-rest.v1:1:{}", "09".repeat(32))),
                version: 1,
                algorithm: KeyAlgorithm::Aes256Gcm,
                fingerprint: None,
            }],
            ..Default::default()
        }
        .with_purpose("pii", "test.layer-four.at-rest.v1");

        let err = KeyManager::from_settings(&settings, None)
            .await
            .expect_err("another purpose's key must not load");
        assert!(matches!(err, KeyError::InvalidConfig(_)));
    }

    /// A configured key with no declared purpose, and a declared key nothing configures, both
    /// refuse at once.
    #[tokio::test]
    async fn config_backend_refuses_keys_the_service_did_not_declare() {
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Config,
            config_keys: vec![ConfigKeyRef {
                id: "pii".to_string(),
                material: Secret::new(format!("test.at-rest.v1:1:{}", "09".repeat(32))),
                version: 1,
                algorithm: KeyAlgorithm::Aes256Gcm,
                fingerprint: None,
            }],
            ..Default::default()
        }
        .with_purpose("mail", "test.mail.v1");

        let err = KeyManager::from_settings(&settings, None)
            .await
            .expect_err("undeclared keys must refuse");
        let KeyError::InvalidConfig(problem) = err else {
            panic!("expected InvalidConfig, got {err:?}");
        };
        assert!(problem.contains("'pii' has no purpose"), "{problem}");
        assert!(problem.contains("expects key 'mail'"), "{problem}");
    }

    #[tokio::test]
    #[traced_test]
    async fn loading_logs_each_key_s_fingerprint_and_never_its_material() {
        let hex = "09".repeat(32);
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Config,
            config_keys: vec![ConfigKeyRef {
                id: "pii".to_string(),
                material: Secret::new(format!("test.at-rest.v1:1:{hex}")),
                version: 1,
                algorithm: KeyAlgorithm::Aes256Gcm,
                fingerprint: None,
            }],
            ..Default::default()
        }
        .with_purpose("pii", "test.at-rest.v1");

        KeyManager::from_settings(&settings, None)
            .await
            .expect("the configured key loads");

        let fingerprint: String = crate::material_fingerprint(&[9u8; 32])
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert!(logs_contain("encryption key loaded"));
        assert!(logs_contain(&fingerprint));
        assert!(!logs_contain(&hex));
    }

    #[test]
    fn a_fingerprint_matches_a_keyring_holding_the_same_material() {
        use crate::{KeyPurpose, Keyring, KeyringKeySettings, KeyringSettings};
        use fundamental_core::secrets::InMemorySecretString;

        struct Probe;
        impl KeyPurpose for Probe {
            const NAME: &'static str = "test.probe";
            const MAX_PLAINTEXT_LEN: usize = 0;
        }
        let keyring = Keyring::<Probe>::from_settings(&KeyringSettings {
            current: 1,
            versions: vec![KeyringKeySettings {
                version: 1,
                material: InMemorySecretString::new(format!("test.probe:1:{}", "5a".repeat(32))),
            }],
        })
        .expect("a valid keyring");
        let manager = KeyManager::builder()
            .with_local_key("pii", &[0x5a_u8; 32])
            .build()
            .expect("a valid manager");

        let from_keyring: Vec<[u8; 32]> = keyring.fingerprints().map(|(_, f)| f).collect();
        let from_manager: Vec<[u8; 32]> = manager
            .fingerprints()
            .into_iter()
            .map(|(_, _, f)| f)
            .collect();
        assert_eq!(from_keyring, from_manager);
    }

    #[tokio::test]
    async fn config_backend_without_keys_errors() {
        let settings = EncryptionSettings {
            enabled: true,
            backend: KeyBackend::Config,
            ..Default::default()
        };

        let err = KeyManager::from_settings(&settings, None)
            .await
            .expect_err("an empty config backend must error");
        assert!(matches!(err, KeyError::InvalidConfig(_)));
    }
}

#[cfg(test)]
mod outbox_seal_tests {
    use super::*;
    use fundamental_core::outbox::RefusalKind;

    const TABLE: &str = "audit_outboxes";

    fn manager_holding(versions: &[(u32, u8)]) -> KeyManager {
        versions
            .iter()
            .fold(KeyManager::builder(), |builder, (version, byte)| {
                builder.with_key_version(
                    KeyManager::OUTBOX_SEAL_KEY,
                    *version,
                    &[*byte; 32],
                    EncryptionAlgorithm::ChaCha20Poly1305,
                )
            })
            .build()
            .expect("a seal key builds")
    }

    fn manager_for_purpose(purpose: &str) -> KeyManager {
        let mut builder = KeyManager::builder();
        builder.add_key_version(
            KeyManager::OUTBOX_SEAL_KEY,
            1,
            &[7; 32],
            EncryptionAlgorithm::ChaCha20Poly1305,
            purpose,
        );
        builder.build().expect("a seal key builds")
    }

    fn row(commitment: &str, leaf: Option<&[u8]>) -> Vec<(&'static str, Option<Vec<u8>>)> {
        vec![
            ("id", Some(b"\x02row-1".to_vec())),
            ("commitment", Some(commitment.as_bytes().to_vec())),
            ("leaf_index", leaf.map(<[u8]>::to_vec)),
        ]
    }

    #[test]
    fn a_seal_checks_against_the_row_it_was_made_for() {
        let manager = manager_holding(&[(1, 1)]);
        let seal = manager
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals");
        assert_eq!(&seal[..4], &1_u32.to_be_bytes());
        manager
            .check_outbox_seal(&seal, TABLE, &row("leaf-a", None))
            .expect("the row it was made for checks");
    }

    #[test]
    fn a_swapped_sealed_value_is_refused_as_a_failed_decryption() {
        let manager = manager_holding(&[(1, 1)]);
        let seal = manager
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals");
        let error = manager
            .check_outbox_seal(&seal, TABLE, &row("leaf-b", None))
            .expect_err("a swapped commitment must not check");
        assert!(matches!(error, KeyError::Decryption(_)), "{error:?}");
        assert_eq!(error.refusal(), Some(RefusalKind::DecryptRefused));
    }

    #[test]
    fn a_nulled_part_and_an_empty_one_seal_apart() {
        let manager = manager_holding(&[(1, 1)]);
        let nulled = manager
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals");
        assert!(
            manager
                .check_outbox_seal(&nulled, TABLE, &row("leaf-a", Some(b"")))
                .is_err_and(|error| error.refusal() == Some(RefusalKind::DecryptRefused)),
            "a NULL column filled with an empty value must change the seal"
        );
    }

    #[test]
    fn the_order_parts_are_named_in_does_not_change_the_seal() {
        let manager = manager_holding(&[(1, 1)]);
        let seal = manager
            .seal_outbox_row(TABLE, &row("leaf-a", Some(b"\x03seven")))
            .expect("seals");
        let mut reordered = row("leaf-a", Some(b"\x03seven"));
        reordered.reverse();
        manager
            .check_outbox_seal(&seal, TABLE, &reordered)
            .expect("the same named parts check in any order");
    }

    #[test]
    fn a_seal_made_for_another_table_does_not_check() {
        let manager = manager_holding(&[(1, 1)]);
        let seal = manager
            .seal_outbox_row("resolution_events", &row("leaf-a", None))
            .expect("seals");
        assert!(
            manager
                .check_outbox_seal(&seal, TABLE, &row("leaf-a", None))
                .is_err_and(|error| error.refusal() == Some(RefusalKind::DecryptRefused))
        );
    }

    #[test]
    fn a_seal_under_another_purpose_does_not_check() {
        let seal = manager_for_purpose("layer0.outbox_seal")
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals");
        assert!(
            manager_for_purpose("layer2.outbox_seal")
                .check_outbox_seal(&seal, TABLE, &row("leaf-a", None))
                .is_err_and(|error| error.refusal() == Some(RefusalKind::DecryptRefused)),
            "one material under two purposes must not make one seal"
        );
    }

    #[test]
    fn a_seal_under_a_version_not_held_here_is_a_key_this_process_lacks() {
        let seal = manager_holding(&[(1, 1), (2, 2)])
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals under the current version");
        assert_eq!(&seal[..4], &2_u32.to_be_bytes());
        let error = manager_holding(&[(1, 1)])
            .check_outbox_seal(&seal, TABLE, &row("leaf-a", None))
            .expect_err("a replica that has not rolled the rotation cannot check it");
        assert!(
            matches!(error, KeyError::KeyVersionNotFound { version: 2, .. }),
            "{error:?}"
        );
        assert_eq!(error.refusal(), Some(RefusalKind::KeyNotHeld));
    }

    #[test]
    fn a_seal_under_a_retained_version_still_checks_after_a_rotation() {
        let seal = manager_holding(&[(1, 1)])
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect("seals");
        manager_holding(&[(1, 1), (2, 2)])
            .check_outbox_seal(&seal, TABLE, &row("leaf-a", None))
            .expect("a rotated manager still checks what the old version sealed");
    }

    #[test]
    fn a_value_that_is_not_a_seal_is_refused_as_a_broken_envelope() {
        let manager = manager_holding(&[(1, 1)]);
        for stored in [&[][..], &[0_u8; 35][..], &[0_u8; 37][..]] {
            let error = manager
                .check_outbox_seal(stored, TABLE, &row("leaf-a", None))
                .expect_err("a value of the wrong length is not a seal");
            assert!(matches!(error, KeyError::Serialization(_)), "{error:?}");
            assert_eq!(error.refusal(), Some(RefusalKind::EnvelopeRefused));
        }
    }

    #[test]
    fn a_set_aside_seal_checks_against_the_decision_it_was_made_for() {
        let manager = manager_holding(&[(1, 1)]);
        let decision = row("decrypt_refused", Some(b"\x06instant"));
        let seal = manager
            .seal_outbox_set_aside(TABLE, &decision)
            .expect("seals");
        manager
            .check_outbox_set_aside(&seal, TABLE, &decision)
            .expect("the decision it was made for checks");
        assert!(
            manager
                .check_outbox_set_aside(&seal, TABLE, &row("value_refused", Some(b"\x06instant")))
                .is_err_and(|error| error.refusal() == Some(RefusalKind::DecryptRefused)),
            "a reason changed by hand must not check"
        );
    }

    #[test]
    fn a_row_seal_and_a_set_aside_seal_never_stand_for_each_other() {
        let manager = manager_holding(&[(1, 1)]);
        let parts = row("leaf-a", None);
        let row_seal = manager.seal_outbox_row(TABLE, &parts).expect("seals");
        let set_aside_seal = manager.seal_outbox_set_aside(TABLE, &parts).expect("seals");
        assert_ne!(row_seal, set_aside_seal);
        assert!(
            manager
                .check_outbox_set_aside(&row_seal, TABLE, &parts)
                .is_err()
        );
        assert!(
            manager
                .check_outbox_seal(&set_aside_seal, TABLE, &parts)
                .is_err()
        );
    }

    #[test]
    fn a_manager_without_a_seal_key_can_neither_seal_nor_check() {
        let manager = KeyManager::builder()
            .with_local_key("pii", &[9; 32])
            .build()
            .expect("builds");
        let error = manager
            .seal_outbox_row(TABLE, &row("leaf-a", None))
            .expect_err("no seal key");
        assert!(matches!(error, KeyError::KeyNotFound(_)), "{error:?}");
        let error = manager
            .check_outbox_seal(
                &[0; KeyManager::OUTBOX_SEAL_LEN],
                TABLE,
                &row("leaf-a", None),
            )
            .expect_err("no seal key");
        assert_eq!(error.refusal(), Some(RefusalKind::KeyNotHeld));
    }
}

#[cfg(test)]
mod tag_tests {
    use super::*;

    fn holding(byte: u8) -> KeyManager {
        KeyManager::builder()
            .with_local_key("forms", &[byte; 32])
            .build()
            .expect("a key builds")
    }

    #[test]
    fn a_tag_is_stable_and_differs_by_label_part_and_key() {
        let keys = holding(1);
        let tag = keys.tag("forms", "start", &[b"ab", b"c"]).expect("a tag");
        assert_eq!(
            tag,
            keys.tag("forms", "start", &[b"ab", b"c"]).expect("a tag")
        );
        assert_ne!(
            tag,
            keys.tag("forms", "start", &[b"a", b"bc"]).expect("a tag")
        );
        assert_ne!(
            tag,
            keys.tag("forms", "other", &[b"ab", b"c"]).expect("a tag")
        );
        assert_ne!(
            tag,
            holding(2)
                .tag("forms", "start", &[b"ab", b"c"])
                .expect("a tag")
        );
    }

    #[test]
    fn a_tag_is_never_the_unkeyed_digest() {
        let tag = holding(1)
            .tag("forms", "start", &[b"192.0.2.1"])
            .expect("a tag");
        assert_ne!(tag, *blake3::hash(b"192.0.2.1").as_bytes());
    }

    #[test]
    fn a_key_not_held_tags_nothing() {
        let refused = holding(1).tag("absent", "start", &[b"x"]);
        assert!(matches!(refused, Err(KeyError::KeyNotFound(id)) if id == "absent"));
    }
}
