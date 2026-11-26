//! [`SessionHandleKey`] — the keyring purpose a session handle is digested under.

use fundamental_encryption::KeyPurpose;

/// The keyring purpose a session handle is digested under, so neither a read nor a write of the
/// store can name a handle.
#[derive(Debug, Clone, Copy)]
pub enum SessionHandleKey {}

impl KeyPurpose for SessionHandleKey {
    const NAME: &'static str = "fundamental.session-handle.v1";
    const MAX_PLAINTEXT_LEN: usize = 0;
}
