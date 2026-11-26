use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Errors raised by device key generation, signing, verification, and storage.
///
/// A bad signature is a 401, not a server fault.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeyError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// A device keypair could not be generated.
    #[error(
        code = "KEYS_001",
        message = "Device key generation failed: {0}",
        status = 500
    )]
    Generate(String),

    /// A signature did not verify against the public key and message.
    #[error(
        code = "KEYS_002",
        message = "Signature verification failed",
        status = 401
    )]
    BadSignature,

    /// A key or signature was not the expected size or encoding.
    #[error(
        code = "KEYS_003",
        message = "Malformed key material: {0}",
        status = 400
    )]
    Malformed(String),

    /// Reading or writing the device key in platform storage failed.
    #[error(
        code = "KEYS_004",
        message = "Device key storage failed: {0}",
        status = 500
    )]
    Storage(String),

    /// The signing operation itself failed.
    #[error(code = "KEYS_005", message = "Signing failed: {0}", status = 500)]
    Sign(String),

    /// A server-side custody backend (HSM, enclave, or KMS) failed; never leaks key material
    /// or a PIN.
    #[error(
        code = "KEYS_006",
        message = "Signing custody backend failed: {0}",
        status = 500
    )]
    Custody(String),

    /// A dual-control signature was refused, so no operator signature was produced.
    #[error(
        code = "KEYS_007",
        message = "Dual-control signing refused: {0}",
        status = 403
    )]
    DualControl(String),

    /// A network custody backend did not answer in time; retryable.
    #[error(
        code = "KEYS_008",
        message = "The signing service is temporarily unavailable",
        status = 503,
        retryable
    )]
    CustodyUnavailable,

    /// A custody backend holds a key of a different algorithm than the signer expects.
    #[error(
        code = "KEYS_009",
        message = "The signing key does not match the expected algorithm",
        status = 500
    )]
    AlgorithmMismatch,

    /// A custody backend reports that the key can be exported, so it cannot hold sole control.
    #[error(
        code = "KEYS_010",
        message = "An exportable key cannot back this signer",
        status = 500
    )]
    ExportableKeyRefused,

    /// A custody backend returned a signature the pinned public key does not verify, so it is
    /// withheld.
    #[error(
        code = "KEYS_011",
        message = "The signing service returned a signature that did not verify",
        status = 500
    )]
    SignatureSelfCheckFailed,

    /// The machine has a TPM this account cannot open (on Linux, usually outside the `tss`
    /// group); refused rather than enrolling the device with a software key for good.
    #[error(
        code = "KEYS_012",
        message = "This computer has a TPM that this account cannot use. Add the account to the `tss` group, then open the app again",
        developer_message = "{path} is present and cannot be opened: {reason}",
        status = 500
    )]
    TpmUnreachable {
        /// The device that answered.
        path: &'static str,
        /// Why it could not be opened.
        reason: String,
    },

    /// A BIP-39 recovery phrase failed validation; the message never echoes a word.
    #[cfg(feature = "keys-hd")]
    #[error(
        code = "KEYS_HD_001",
        message = "Recovery phrase is not a valid BIP-39 mnemonic",
        status = 400
    )]
    MnemonicInvalid,

    /// The platform CSPRNG could not produce entropy for a fresh mnemonic; retryable.
    #[cfg(feature = "keys-hd")]
    #[error(
        code = "KEYS_HD_005",
        message = "The entropy source failed while generating a recovery phrase",
        status = 500,
        retryable
    )]
    Entropy,

    /// Sealing the master seed at rest failed; the message never carries a secret.
    #[cfg(feature = "keys-hd")]
    #[error(
        code = "KEYS_HD_003",
        message = "Sealing the seed failed: {0}",
        status = 500
    )]
    Seal(String),

    /// Unsealing the master seed failed; every cause collapses here, so it never says which.
    #[cfg(feature = "keys-hd")]
    #[error(
        code = "KEYS_HD_004",
        message = "Unsealing the seed failed",
        status = 401
    )]
    Unseal,
}
