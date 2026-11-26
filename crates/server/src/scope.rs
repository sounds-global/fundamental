use fundamental_auth::AuthError;
use fundamental_core::{AnyError, Error, ScopeSet};
use tracing::warn;

use crate::http_context::HttpContext;
use crate::middleware::{Middleware, MiddlewareResult};
use crate::next::Next;

/// A scope guard: the request proceeds only if the principal holds every scope the route requires.
///
/// ```ignore
/// #[action(POST, "/erase", auth, scope(Subject::Erase))]
/// ```
pub struct Scope {
    required: &'static [&'static str],
}

/// Builds a guard requiring every scope `set` denotes; list it twice to span vocabularies.
#[must_use]
pub fn scope<S: ScopeSet>(set: S) -> Scope {
    Scope {
        required: set.required_scopes(),
    }
}

impl<S> Middleware<S> for Scope
where
    S: Send + Sync + 'static,
{
    // A scope is only ever held by a principal, so the route is unsatisfiable without a verifier.
    const NEEDS_PRINCIPAL: bool = true;

    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        // Scoped so the principal borrow ends before `ctx` moves into the chain.
        let missing = {
            let principal = ctx.principal().map_err(AnyError::new)?;
            self.required
                .iter()
                .find(|scope| !principal.has_scope(scope))
                .map(|scope| ((*scope).to_owned(), principal.user_id.to_string()))
        };

        if let Some((scope, user_id)) = missing {
            // The auth crate's error, so the refusal reads the same over HTTP and over a message bus.
            let error = AuthError::InsufficientScope { scope };
            // The scope and the subject, never the token.
            warn!(
                error = error.error_code(),
                user_id, "rejected request lacking a required scope"
            );
            return Err(AnyError::new(error));
        }

        next.run(ctx, body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Route;
    use fundamental_auth::{AuthenticatedPrincipal, UserId};

    #[derive(Clone)]
    struct State;

    enum Subject {
        Erase,
    }

    impl ScopeSet for Subject {
        fn required_scopes(&self) -> &'static [&'static str] {
            match self {
                Self::Erase => &["subject:erase"],
            }
        }
    }

    enum Both {
        EraseAndAudit,
    }

    impl ScopeSet for Both {
        fn required_scopes(&self) -> &'static [&'static str] {
            match self {
                Self::EraseAndAudit => &["subject:erase", "audit:write"],
            }
        }
    }

    fn context(scopes: &[&str]) -> HttpContext<State> {
        let ctx = HttpContext::new(State, "req-1");
        ctx.with_principal(Some(AuthenticatedPrincipal {
            user_id: UserId::new("ops:dpo"),
            device_id: None,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            expires_at: 0,
        }))
    }

    fn guarded<S: ScopeSet>(set: S) -> Route<State> {
        Route::<State>::post("/erase", |_ctx, _input: ()| async move { Ok(()) })
            .middleware(scope(set))
    }

    #[tokio::test]
    async fn a_principal_holding_the_scope_proceeds() {
        let route = guarded(Subject::Erase);
        route
            .call(context(&["subject:erase"]), Vec::new())
            .await
            .expect("the guard admits a principal holding the scope");
    }

    #[tokio::test]
    async fn a_principal_holding_no_scopes_is_forbidden() {
        let route = guarded(Subject::Erase);
        let err = route
            .call(context(&[]), Vec::new())
            .await
            .expect_err("no scopes");
        assert_eq!(err.get().error_code(), "AUTH_008");
        assert_eq!(err.get().http_status_code(), 403);
    }

    #[tokio::test]
    async fn a_principal_holding_a_different_scope_is_forbidden() {
        let route = guarded(Subject::Erase);
        let err = route
            .call(context(&["credential:revoke"]), Vec::new())
            .await
            .expect_err("wrong scope");
        assert_eq!(err.get().error_code(), "AUTH_008");
    }

    /// Holding *some* of a conjunction is not holding it.
    #[tokio::test]
    async fn a_partial_match_on_a_conjunction_is_forbidden() {
        let route = guarded(Both::EraseAndAudit);
        let err = route
            .call(context(&["subject:erase"]), Vec::new())
            .await
            .expect_err("only one of two");
        assert_eq!(err.get().error_code(), "AUTH_008");
        assert!(
            err.to_string().contains("audit:write"),
            "the message must name the scope that is missing: {err}"
        );
    }

    #[tokio::test]
    async fn every_scope_of_a_conjunction_admits() {
        let route = guarded(Both::EraseAndAudit);
        route
            .call(context(&["subject:erase", "audit:write"]), Vec::new())
            .await
            .expect("holds both");
    }

    #[tokio::test]
    async fn scope_matching_is_case_sensitive() {
        let route = guarded(Subject::Erase);
        let err = route
            .call(context(&["Subject:Erase"]), Vec::new())
            .await
            .expect_err("case differs");
        assert_eq!(err.get().error_code(), "AUTH_008");
    }

    #[tokio::test]
    async fn no_principal_is_unauthenticated_not_forbidden() {
        let route = guarded(Subject::Erase);
        let err = route
            .call(HttpContext::new(State, "req-1"), Vec::new())
            .await
            .expect_err("no principal");
        assert_eq!(err.get().error_code(), "API_009");
        assert_eq!(err.get().http_status_code(), 401);
    }

    #[test]
    fn a_scope_guard_declares_that_it_needs_a_principal() {
        let route = guarded(Subject::Erase);
        assert!(route.needs_principal());
    }
}
