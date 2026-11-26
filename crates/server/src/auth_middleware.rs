use fundamental_core::{AnyError, Error};
use fundamental_macros::middleware;
use tracing::warn;

use crate::http_context::HttpContext;
use crate::next::Next;
use crate::{MiddlewareResult, ServerError};

/// Middleware that rejects a request with no authenticated principal.
///
/// A rejected request returns [`ServerError::Unauthenticated`] (`API_009`, `401`).
#[middleware(needs_principal)]
pub async fn auth<S>(ctx: HttpContext<S>, body: Vec<u8>, next: Next<S>) -> MiddlewareResult {
    if ctx.principal_opt().is_none() {
        let error = ServerError::unauthenticated();
        warn!(
            error = error.error_code(),
            "rejected unauthenticated request"
        );
        return Err(AnyError::new(error));
    }
    next.run(ctx, body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Route;
    use fundamental_auth::{AuthenticatedPrincipal, UserId};

    #[derive(Clone)]
    struct State;

    fn principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            user_id: UserId::new("u1"),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }
    }

    fn route() -> Route<State> {
        Route::get("/me", |_ctx, _input: ()| async move { Ok(()) }).middleware(auth)
    }

    #[tokio::test]
    async fn rejects_when_no_principal() {
        let ctx = HttpContext::new(State, "req-1");
        let err = route().call(ctx, Vec::new()).await.expect_err("gated");
        assert_eq!(err.get().error_code(), "API_009");
        assert_eq!(err.get().http_status_code(), 401);
    }

    #[tokio::test]
    async fn calls_through_with_a_principal() {
        let ctx = HttpContext::new(State, "req-1").with_principal(Some(principal()));
        let body = route().call(ctx, Vec::new()).await.expect("passes gate");
        assert_eq!(body, b"null");
    }
}
