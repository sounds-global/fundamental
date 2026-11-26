use core::fmt;

use serde::{Deserialize, Serialize};

use crate::{Encrypted, EncryptedEnvelope, EncryptionMetadata};

/// The ciphertext carrier behind a `SecretBytes` field; not public API.
#[doc(hidden)]
#[derive(Clone)]
pub struct EncryptedBytes {
    /// The encrypted data.
    pub ciphertext: Vec<u8>,
    /// Encryption metadata.
    pub metadata: EncryptionMetadata,
}

impl EncryptedBytes {
    /// Creates new encrypted bytes from ciphertext and metadata.
    pub fn from_encrypted(ciphertext: Vec<u8>, metadata: EncryptionMetadata) -> Self {
        Self {
            ciphertext,
            metadata,
        }
    }

    /// Creates encrypted bytes from an envelope.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn from_envelope(envelope: EncryptedEnvelope) -> Result<Self, &'static str> {
        let ciphertext = envelope
            .decode_ciphertext()
            .map_err(|_| "invalid ciphertext encoding")?;
        let metadata = envelope.to_metadata()?;
        Ok(Self {
            ciphertext,
            metadata,
        })
    }

    /// Converts to an encrypted envelope for serialization.
    pub fn to_envelope(&self) -> EncryptedEnvelope {
        EncryptedEnvelope::new(&self.ciphertext, &self.metadata)
    }

    /// Returns the key identifier used for encryption.
    pub fn key_id(&self) -> &str {
        &self.metadata.key_id
    }

    /// Returns the key version used for encryption.
    pub fn key_version(&self) -> u32 {
        self.metadata.key_version
    }
}

impl Encrypted for EncryptedBytes {
    type Plaintext = Vec<u8>;

    fn metadata(&self) -> &EncryptionMetadata {
        &self.metadata
    }

    fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

impl fmt::Debug for EncryptedBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedBytes")
            .field("key_id", &self.metadata.key_id)
            .field("key_version", &self.metadata.key_version)
            .field("ciphertext_len", &self.ciphertext.len())
            .finish()
    }
}

impl Serialize for EncryptedBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_envelope().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EncryptedBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let envelope = EncryptedEnvelope::deserialize(deserializer)?;
        Self::from_envelope(envelope).map_err(serde::de::Error::custom)
    }
}

impl PartialEq for EncryptedBytes {
    fn eq(&self, other: &Self) -> bool {
        self.ciphertext == other.ciphertext
            && self.metadata.key_id == other.metadata.key_id
            && self.metadata.key_version == other.metadata.key_version
    }
}

impl Eq for EncryptedBytes {}
