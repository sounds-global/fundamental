/// Where a [`DeviceKey`](super::DeviceKey) is kept, as the device can state it.
///
/// Only [`Tpm`](Self::Tpm) can be proved, through attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCustody {
    /// A software key sealed in the OS key store; readable by malware running as the user.
    SealedInKeyStore,
    /// A key a TPM generated and never releases.
    Tpm,
}
