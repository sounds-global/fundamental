use fundamental_core::secrets::Secret;

use crate::hd::VaultOwnerId;
use crate::{Signature, SigningKey, VerifyingKey};

/// The domain tag for a vault-ownership challenge signature, so it replays as no other signature.
pub const VAULT_AUTH_DOMAIN: &[u8] = b"fundamental/vault/auth-v1";

/// The Ed25519 keypair that authenticates a device to its E2EE vault hub.
///
/// Derived from the seed, so every device holding it derives the same key and possession of the
/// seed is ownership.
#[derive(Clone)]
pub struct VaultOwnerKey {
    signing: SigningKey,
}

impl VaultOwnerKey {
    /// Reconstructs the owner keypair from the vault **owner-auth child seed**.
    #[must_use]
    pub fn from_child_seed(child_seed: &Secret<[u8; 32]>) -> Self {
        Self {
            signing: SigningKey::from_seed(child_seed),
        }
    }

    /// The public key a server verifies challenge signatures against.
    #[must_use]
    pub fn public_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// The blinded owner id this key commits to — the hub's partition key.
    #[must_use]
    pub fn owner_id(&self) -> VaultOwnerId {
        VaultOwnerId::from_public_key(&self.public_key())
    }

    /// Signs a server-issued single-use `nonce` as `VAULT_AUTH_DOMAIN ‖ nonce`.
    #[must_use]
    pub fn sign_challenge(&self, nonce: &[u8]) -> Signature {
        let mut message = Vec::with_capacity(VAULT_AUTH_DOMAIN.len() + nonce.len());
        message.extend_from_slice(VAULT_AUTH_DOMAIN);
        message.extend_from_slice(nonce);
        self.signing.sign(&message)
    }
}

/// Verifies that `signature` is `public_key`'s over `VAULT_AUTH_DOMAIN ‖ nonce`.
///
/// # Errors
/// [`KeyError::BadSignature`](crate::KeyError::BadSignature) if the signature does not
/// verify, or [`KeyError::Malformed`](crate::KeyError::Malformed) if `public_key` is not a point.
pub fn verify_owner_challenge(
    public_key: &VerifyingKey,
    nonce: &[u8],
    signature: &Signature,
) -> Result<(), crate::KeyError> {
    let mut message = Vec::with_capacity(VAULT_AUTH_DOMAIN.len() + nonce.len());
    message.extend_from_slice(VAULT_AUTH_DOMAIN);
    message.extend_from_slice(nonce);
    public_key.verify(&message, signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyError;

    fn owner_key(seed: [u8; 32]) -> VaultOwnerKey {
        VaultOwnerKey::from_child_seed(&Secret::new(seed))
    }

    #[test]
    fn same_seed_derives_the_same_owner() {
        let a = owner_key([0x41u8; 32]);
        let b = owner_key([0x41u8; 32]);
        assert_eq!(a.owner_id(), b.owner_id());
        assert_eq!(a.public_key(), b.public_key());
    }

    #[test]
    fn distinct_seeds_derive_distinct_owners() {
        assert_ne!(
            owner_key([1u8; 32]).owner_id(),
            owner_key([2u8; 32]).owner_id()
        );
    }

    #[test]
    fn a_valid_challenge_response_verifies() {
        let key = owner_key([0x51u8; 32]);
        let nonce = [0x99u8; 32];
        let sig = key.sign_challenge(&nonce);
        verify_owner_challenge(&key.public_key(), &nonce, &sig).expect("owner proof verifies");
    }

    #[test]
    fn a_response_to_a_different_nonce_fails_closed() {
        let key = owner_key([0x52u8; 32]);
        let sig = key.sign_challenge(&[0x01u8; 32]);
        assert!(matches!(
            verify_owner_challenge(&key.public_key(), &[0x02u8; 32], &sig),
            Err(KeyError::BadSignature)
        ));
    }

    #[test]
    fn another_owners_key_cannot_answer_the_challenge() {
        let owner = owner_key([0x53u8; 32]);
        let attacker = owner_key([0x54u8; 32]);
        let nonce = [0x77u8; 32];
        let sig = attacker.sign_challenge(&nonce);
        assert!(matches!(
            verify_owner_challenge(&owner.public_key(), &nonce, &sig),
            Err(KeyError::BadSignature)
        ));
    }
}
