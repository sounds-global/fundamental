use fundamental_core::secrets::InMemorySecretString;

use crate::server::KeyId;

/// Where a non-exportable signing key lives in a PKCS#11 module, and how to authenticate to it.
///
/// The key is found by `token_label` and `key_label`, never a slot index, so it survives
/// renumbering.
pub struct Pkcs11Config {
    /// The logical identifier the signer reports for this key.
    pub key_id: KeyId,
    /// Filesystem path to the PKCS#11 module (e.g. `libsofthsm2.so`, a vendor `.so`).
    pub module_path: String,
    /// The token label the signing key lives on.
    pub token_label: String,
    /// The `CKA_LABEL` of the signing key pair.
    pub key_label: String,
    /// The never-persisted user PIN used to log in to the token.
    pub pin: InMemorySecretString,
}
