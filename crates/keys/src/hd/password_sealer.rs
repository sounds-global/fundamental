use argon2::Argon2;
use fundamental_core::secrets::SecretString;
use zeroize::Zeroize;

use crate::KeyError;
use crate::hd::seal_aead::{seal_seed, unseal_seed};
use crate::hd::{MasterSeed, SealedSeed, SeedSealer};

const SALT_LEN: usize = 16;

/// A [`SeedSealer`] that wraps the master seed under a key derived from a password by
/// **Argon2id**, with a random per-seal salt stored in the [`SealedSeed`].
pub struct PasswordSealer {
    password: SecretString,
}

impl PasswordSealer {
    /// Creates a sealer over `password`.
    #[must_use]
    pub fn new(password: SecretString) -> Self {
        Self { password }
    }

    fn derive_key(&self, salt: &[u8]) -> Result<[u8; 32], KeyError> {
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(self.password.expose().as_bytes(), salt, &mut key)
            .map_err(|_| KeyError::Seal("argon2 key derivation failed".into()))?;
        Ok(key)
    }
}

impl SeedSealer for PasswordSealer {
    async fn seal(&self, seed: &MasterSeed) -> Result<SealedSeed, KeyError> {
        let mut salt = [0u8; SALT_LEN];
        getrandom::getrandom(&mut salt)
            .map_err(|_| KeyError::Seal("salt entropy failed".into()))?;
        let mut key = self.derive_key(&salt)?;
        let sealed = seal_seed(&key, salt.to_vec(), seed);
        key.zeroize();
        sealed
    }

    async fn unseal(&self, sealed: &SealedSeed) -> Result<MasterSeed, KeyError> {
        let mut key = self.derive_key(sealed.salt())?;
        let recovered = unseal_seed(&key, sealed);
        key.zeroize();
        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealer(pw: &str) -> PasswordSealer {
        PasswordSealer::new(SecretString::new(pw.to_string()))
    }

    #[tokio::test]
    async fn round_trips_the_seed() {
        let seed = MasterSeed::from_bytes([0x24u8; 64]);
        let sealer = sealer("correct horse battery staple");
        let sealed = sealer.seal(&seed).await.expect("seal");
        let recovered = sealer.unseal(&sealed).await.expect("unseal");
        assert_eq!(recovered.expose(), seed.expose());
    }

    #[tokio::test]
    async fn a_wrong_password_fails_closed() {
        let seed = MasterSeed::from_bytes([0x24u8; 64]);
        let sealed = sealer("right-password").seal(&seed).await.expect("seal");
        let err = sealer("wrong-password")
            .unseal(&sealed)
            .await
            .expect_err("wrong password must fail");
        assert!(matches!(err, KeyError::Unseal));
    }

    #[tokio::test]
    async fn each_seal_uses_a_fresh_salt() {
        let seed = MasterSeed::from_bytes([0x24u8; 64]);
        let sealer = sealer("pw");
        let a = sealer.seal(&seed).await.expect("a");
        let b = sealer.seal(&seed).await.expect("b");
        assert_ne!(
            a.to_bytes(),
            b.to_bytes(),
            "independent salts → distinct envelopes"
        );
        assert_eq!(sealer.unseal(&a).await.expect("ua").expose(), seed.expose());
        assert_eq!(sealer.unseal(&b).await.expect("ub").expose(), seed.expose());
    }
}
