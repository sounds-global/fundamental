//! [`PrincipalVerifier`] — the seam for turning a token into a principal.

use std::future::Future;

use fundamental_core::secrets::SecretString;

use crate::{AuthError, AuthenticatedPrincipal};

/// Verifies a presented token and returns the [`AuthenticatedPrincipal`] it proves.
pub trait PrincipalVerifier {
    /// Verifies `token` and returns the principal it authenticates.
    ///
    /// # Errors
    /// Returns an [`AuthError`] if the token is missing, invalid, expired, or untrusted.
    fn verify(
        &self,
        token: &SecretString,
    ) -> impl Future<Output = Result<AuthenticatedPrincipal, AuthError>> + Send;
}

/// A shared verifier verifies, so one key refresh serves several surfaces.
impl<V> PrincipalVerifier for std::sync::Arc<V>
where
    V: PrincipalVerifier + Send + Sync,
{
    fn verify(
        &self,
        token: &SecretString,
    ) -> impl Future<Output = Result<AuthenticatedPrincipal, AuthError>> + Send {
        V::verify(self, token)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::UserId;

    struct Stub;

    impl PrincipalVerifier for Stub {
        async fn verify(&self, _token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
            Ok(AuthenticatedPrincipal {
                user_id: UserId::new("u1"),
                device_id: None,
                scopes: Vec::new(),
                expires_at: 0,
            })
        }
    }

    #[tokio::test]
    async fn an_arc_verifier_verifies_through_its_inner() {
        let shared = Arc::new(Stub);
        let second = Arc::clone(&shared);
        let token = SecretString::new("tok".to_string());

        for verifier in [shared, second] {
            let principal = verifier.verify(&token).await.expect("verifies");
            assert_eq!(principal.user_id.as_str(), "u1");
        }
    }
}
