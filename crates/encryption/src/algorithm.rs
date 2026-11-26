/// Supported encryption algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (Galois/Counter Mode).
    #[default]
    Aes256Gcm,

    /// ChaCha20-Poly1305.
    ChaCha20Poly1305,
}
