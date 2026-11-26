/// What a [`Keyring`](crate::Keyring)'s keys are for.
///
/// One zero-sized type per purpose. `NAME` is mixed into every subkey and token and must label
/// every configured key material; changing it invalidates everything the keyring produced.
///
/// ```
/// use fundamental_encryption::KeyPurpose;
///
/// /// Handles a resolver seals for the issuer and opens itself.
/// pub struct LiveHandle;
///
/// impl KeyPurpose for LiveHandle {
///     const NAME: &'static str = "acme.live-handle.v1";
///     const MAX_PLAINTEXT_LEN: usize = 96;
/// }
///
/// assert_eq!(LiveHandle::NAME, "acme.live-handle.v1");
/// ```
pub trait KeyPurpose: 'static {
    /// The purpose's stable, globally unique name.
    const NAME: &'static str;

    /// The longest plaintext this purpose seals.
    ///
    /// [`Keyring::open`](crate::Keyring::open) refuses a longer token before any cryptographic work.
    const MAX_PLAINTEXT_LEN: usize;
}
