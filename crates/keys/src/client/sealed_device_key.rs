use std::fmt;

use fundamental_core::secrets::SecretString;
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{DerSignature, SigningKey};
use p256::pkcs8::{DecodePrivateKey as _, EncodePrivateKey as _, EncodePublicKey as _, LineEnding};
use tracing::{info, instrument};

use super::{DeviceKey, KeyCustody};
use crate::{KeyError, SecretStore};

/// A P-256 key generated here and sealed in a [`SecretStore`] — on a desktop, the OS key store
/// (`KeyringSecretStore`, feature `os-keychain`).
///
/// Held in memory while it signs and zeroized on drop; as strong as the user account.
pub struct SealedDeviceKey {
    signing: SigningKey,
    public: Vec<u8>,
}

impl SealedDeviceKey {
    /// The key sealed in `store` under `account`, generated and sealed first if there is none.
    ///
    /// # Errors
    /// [`KeyError::Storage`] when the store cannot be read or written, and
    /// [`KeyError::Malformed`] when what it holds is not a P-256 key.
    #[instrument(skip(store), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn open_or_create(store: &dyn SecretStore, account: &str) -> Result<Self, KeyError> {
        let signing = match store.load(account)? {
            Some(sealed) => SigningKey::from_pkcs8_pem(sealed.expose()).map_err(|error| {
                KeyError::Malformed(format!("the sealed device key is not P-256: {error}"))
            })?,
            None => {
                let signing = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
                let pem = signing
                    .to_pkcs8_pem(LineEnding::LF)
                    .map_err(|error| KeyError::Generate(error.to_string()))?;
                store.store(account, &SecretString::new(pem.to_string()))?;
                info!("generated and sealed a new device key");
                signing
            }
        };
        let public = signing
            .verifying_key()
            .to_public_key_der()
            .map_err(|error| KeyError::Malformed(error.to_string()))?
            .into_vec();
        Ok(Self { signing, public })
    }
}

impl DeviceKey for SealedDeviceKey {
    fn public_key_der(&self) -> &[u8] {
        &self.public
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyError> {
        let signature: DerSignature = self.signing.sign(message);
        Ok(signature.as_bytes().to_vec())
    }

    fn custody(&self) -> KeyCustody {
        KeyCustody::SealedInKeyStore
    }
}

// Manual, so the key never reaches a log.
impl fmt::Debug for SealedDeviceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SealedDeviceKey")
            .field("public_key_der_len", &self.public.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::signature::Verifier as _;
    use p256::ecdsa::{DerSignature, VerifyingKey};
    use p256::pkcs8::DecodePublicKey as _;

    use super::{DeviceKey, KeyCustody, SealedDeviceKey};
    use crate::MemorySecretStore;

    #[test]
    fn a_device_finds_the_key_its_first_run_sealed() {
        let store = MemorySecretStore::new();
        let first = SealedDeviceKey::open_or_create(&store, "computer").expect("created");
        let again = SealedDeviceKey::open_or_create(&store, "computer").expect("found");
        assert_eq!(first.public_key_der(), again.public_key_der());
        let other = SealedDeviceKey::open_or_create(&store, "another").expect("created");
        assert_ne!(first.public_key_der(), other.public_key_der());
        assert_eq!(first.custody(), KeyCustody::SealedInKeyStore);
    }

    #[test]
    fn it_signs_ecdsa_p256_sha256_der_under_its_public_key() {
        let store = MemorySecretStore::new();
        let key = SealedDeviceKey::open_or_create(&store, "computer").expect("created");
        let signature = key.sign(b"a handshake transcript").expect("signed");
        let verifying = VerifyingKey::from_public_key_der(key.public_key_der()).expect("SPKI");
        let signature = DerSignature::from_bytes(&signature).expect("DER");
        verifying
            .verify(b"a handshake transcript", &signature)
            .expect("verifies");
        assert!(verifying.verify(b"another message", &signature).is_err());
    }

    #[test]
    fn its_debug_prints_no_key() {
        let store = MemorySecretStore::new();
        let key = SealedDeviceKey::open_or_create(&store, "computer").expect("created");
        let printed = format!("{key:?}");
        assert!(!printed.contains("BEGIN"), "{printed}");
    }
}
