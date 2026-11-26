//! [`SessionHandle`] — an opaque session credential and the digest it is stored as.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fundamental_core::secrets::SecretString;
use fundamental_encryption::Keyring;

use crate::{AuthError, SessionDigest, SessionHandleKey};

/// A freshly minted session handle: the secret the browser holds, and the keyed digest the server
/// stores.
#[derive(Debug)]
pub struct SessionHandle {
    secret: SecretString,
    digest: SessionDigest,
}

impl SessionHandle {
    /// How many characters a handle is.
    pub const LENGTH: usize = 43;

    /// Mints a new handle, digested under `keys`' current version.
    ///
    /// # Errors
    /// [`AuthError::EntropyUnavailable`] when the OS random source cannot be read.
    pub fn mint(keys: &Keyring<SessionHandleKey>) -> Result<Self, AuthError> {
        let mut bytes = [0_u8; 32];
        getrandom::getrandom(&mut bytes)
            .map_err(|e| AuthError::EntropyUnavailable(e.to_string()))?;
        let spelled = URL_SAFE_NO_PAD.encode(bytes);
        let digest = keys.tag(&[spelled.as_bytes()]);
        Ok(Self {
            secret: SecretString::new(spelled),
            digest,
        })
    }

    /// The digest a presented handle is looked up by, or `None` when it cannot be a handle.
    #[must_use]
    pub fn digest_of(
        keys: &Keyring<SessionHandleKey>,
        presented: &SecretString,
    ) -> Option<SessionDigest> {
        well_formed(presented).then(|| keys.tag(&[presented.expose().as_bytes()]))
    }

    /// Every digest a presented handle may be stored under, current key first, or `None` when it
    /// cannot be a handle.
    #[must_use]
    pub fn digests_of(
        keys: &Keyring<SessionHandleKey>,
        presented: &SecretString,
    ) -> Option<Vec<SessionDigest>> {
        well_formed(presented).then(|| keys.tags(&[presented.expose().as_bytes()]))
    }

    /// The secret the browser holds, to set in its cookie.
    #[must_use]
    pub fn secret(&self) -> &SecretString {
        &self.secret
    }

    /// The digest the store keeps.
    #[must_use]
    pub fn digest(&self) -> SessionDigest {
        self.digest
    }
}

fn well_formed(presented: &SecretString) -> bool {
    let spelled = presented.expose();
    spelled.len() == SessionHandle::LENGTH
        && spelled
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
pub(crate) mod tests {
    use fundamental_core::secrets::InMemorySecretString;
    use fundamental_encryption::{KeyPurpose as _, KeyringKeySettings, KeyringSettings};

    use super::*;

    /// A keyring holding `versions` (each version's material is its number repeated), current `current`.
    pub(crate) fn keys(current: u16, versions: &[u16]) -> Keyring<SessionHandleKey> {
        Keyring::from_settings(&KeyringSettings {
            current,
            versions: versions
                .iter()
                .map(|&version| KeyringKeySettings {
                    version,
                    material: InMemorySecretString::new(format!(
                        "{}:{version}:{}",
                        SessionHandleKey::NAME,
                        format!("{version:02x}").repeat(32)
                    )),
                })
                .collect(),
        })
        .expect("a test keyring")
    }

    #[test]
    fn a_handle_is_digested_as_it_is_presented() {
        let keys = keys(1, &[1]);
        let handle = SessionHandle::mint(&keys).expect("the OS has entropy");
        assert_eq!(handle.secret().expose().len(), SessionHandle::LENGTH);
        let presented = SecretString::new(handle.secret().expose().to_owned());
        assert_eq!(
            SessionHandle::digest_of(&keys, &presented).map(|d| *d.as_bytes()),
            Some(*handle.digest().as_bytes())
        );
    }

    #[test]
    fn two_handles_differ() {
        let keys = keys(1, &[1]);
        let first = SessionHandle::mint(&keys).expect("entropy");
        let second = SessionHandle::mint(&keys).expect("entropy");
        assert_ne!(first.secret().expose(), second.secret().expose());
        assert_ne!(first.digest().as_bytes(), second.digest().as_bytes());
    }

    #[test]
    fn the_stored_form_needs_the_key() {
        let handle = SessionHandle::mint(&keys(1, &[1])).expect("entropy");
        let presented = SecretString::new(handle.secret().expose().to_owned());
        let elsewhere = SessionHandle::digest_of(&keys(2, &[2]), &presented).expect("well formed");
        assert_ne!(elsewhere.as_bytes(), handle.digest().as_bytes());
        assert_ne!(
            handle.digest().as_bytes().as_slice(),
            handle.secret().expose().as_bytes()
        );
    }

    #[test]
    fn a_handle_from_before_a_rotation_is_still_found() {
        let before = SessionHandle::mint(&keys(1, &[1])).expect("entropy");
        let presented = SecretString::new(before.secret().expose().to_owned());
        let rotated =
            SessionHandle::digests_of(&keys(2, &[1, 2]), &presented).expect("well formed");
        assert_eq!(
            rotated.len(),
            2,
            "current first, then every retained version"
        );
        assert!(
            rotated
                .iter()
                .any(|digest| digest.as_bytes() == before.digest().as_bytes())
        );
    }

    #[test]
    fn a_value_that_cannot_be_a_handle_is_not_digested() {
        let keys = keys(1, &[1]);
        for presented in [
            "",
            "short",
            &"a".repeat(44),
            &format!("{}=", "a".repeat(42)),
            &format!("{}.", "a".repeat(42)),
        ] {
            let presented = SecretString::new(presented.to_owned());
            assert!(
                SessionHandle::digest_of(&keys, &presented).is_none(),
                "{presented:?}"
            );
            assert!(SessionHandle::digests_of(&keys, &presented).is_none());
        }
    }

    #[test]
    fn the_debug_form_does_not_reveal_the_secret() {
        let handle = SessionHandle::mint(&keys(1, &[1])).expect("entropy");
        assert!(!format!("{handle:?}").contains(handle.secret().expose()));
    }
}
