use std::future::Future;
use std::pin::Pin;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::AnyError;

use crate::PrincipalSource;

/// A [`PrincipalSource`] that always resolves to no principal.
///
/// The default for [`InProcessBridge`](crate::InProcessBridge): auth-gated routes reject it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anonymous;

impl PrincipalSource for Anonymous {
    fn resolve(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AuthenticatedPrincipal>, AnyError>> + Send + '_>>
    {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(test)]
mod tests {
    use super::Anonymous;
    use crate::PrincipalSource;

    #[tokio::test]
    async fn anonymous_resolves_to_no_principal() {
        let principal = Anonymous.resolve().await.expect("never fails");
        assert!(principal.is_none());
    }
}
