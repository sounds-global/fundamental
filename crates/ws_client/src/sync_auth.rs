//! [`SyncAuth`] — how a [`SyncClient`](crate::SyncClient) authenticates its connection.

#[cfg(feature = "vault-auth")]
use std::sync::Arc;

use fundamental_core::secrets::SecretString;
#[cfg(feature = "vault-auth")]
use fundamental_keys::VaultOwnerKey;

/// The credential a sync connection presents to bind its principal.
#[derive(Clone)]
pub enum SyncAuth {
    /// Bearer/session-token auth presented on the first frame.
    Jwt(SecretString),
    /// Blinded-owner vault auth via the owner-auth challenge/response.
    #[cfg(feature = "vault-auth")]
    Vault(Arc<VaultOwnerKey>),
}

impl std::fmt::Debug for SyncAuth {
    /// Never renders the token or the owner key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jwt(_) => f.write_str("Jwt(…)"),
            #[cfg(feature = "vault-auth")]
            Self::Vault(_) => f.write_str("Vault(…)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_renders_the_jwt() {
        let auth = SyncAuth::Jwt(SecretString::new("super-secret".into()));
        assert_eq!(format!("{auth:?}"), "Jwt(…)");
    }
}
