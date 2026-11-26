/// Whether a custody backend can ever release a key's private material.
///
/// [`KmsSigner`](super::KmsSigner) refuses an [`Exportable`](Self::Exportable) key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exportability {
    /// The backend never releases the private key.
    NonExportable,
    /// The backend can release the private key, or back it up in plaintext.
    Exportable,
}
