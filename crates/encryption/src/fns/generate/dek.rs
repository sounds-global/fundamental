/// Generates a cryptographically secure random encryption key.
///
/// The key is 32 bytes, for AES-256.
///
/// # Examples
///
/// ```no_run
/// use fundamental_encryption::generate_dek;
///
/// // Generate a new 32-byte DEK.
/// let dek = generate_dek();
/// assert_eq!(dek.len(), 32);
///
/// // Encrypt `dek` with your KMS and store the wrapped key in config/secrets.
/// ```
///
/// # Panics
///
/// Panics if the system random number generator fails to produce key material.
pub fn generate_dek() -> Vec<u8> {
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();
    let mut key = vec![0u8; 32];
    rng.fill(&mut key).expect("failed to generate random key");
    key
}
