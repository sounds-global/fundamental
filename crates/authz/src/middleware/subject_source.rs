//! [`SubjectSource`]: who a [`PermissionGuard`](super::PermissionGuard) authorizes as.
//!
//! The mirror of [`ObjectSource`](super::ObjectSource), but async and fallible, since a
//! subject is often a row: `Ok(None)` denies as the public wildcard, and `Err` surfaces as a
//! `500` or `503`, never as a denial.

use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_server::HttpContext;

/// Resolves the subject id a guarded request is authorized as.
///
/// `Ok(None)` is checked as the public wildcard; `Err` must never read as a denial.
pub trait SubjectSource<S>: Send + Sync + 'static {
    /// The subject id for this request, if one can be resolved.
    ///
    /// # Errors
    /// [`AnyError`] when resolving the subject failed; never for a caller who holds nothing.
    fn subject_id<'a>(
        &'a self,
        ctx: &'a HttpContext<S>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, AnyError>> + Send + 'a>>;

    /// How this source is addressed, for the diagnostic when it resolves nothing.
    fn describe(&self) -> String;
}

/// The subject is the request's authenticated principal, verbatim.
///
/// The default.
#[derive(Debug, Default)]
pub struct PrincipalSubject;

impl<S> SubjectSource<S> for PrincipalSubject
where
    S: Send + Sync + 'static,
{
    fn subject_id<'a>(
        &'a self,
        ctx: &'a HttpContext<S>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, AnyError>> + Send + 'a>> {
        let id = ctx
            .principal_opt()
            .map(|principal| principal.user_id.as_str().to_string());
        Box::pin(async move { Ok(id) })
    }

    fn describe(&self) -> String {
        "the request's principal".to_string()
    }
}

/// The subject is resolved from the request by a caller-supplied function.
///
/// The closure receives the whole [`HttpContext`] and may await. Return `Ok(None)` for a caller
/// who holds nothing and `Err` for a lookup that failed. The future is `'static`, so take
/// what it needs from the request before the first `await`.
pub struct ResolvedSubject<F> {
    pub(super) resolve: F,
    pub(super) described_as: &'static str,
}

impl<S, F, Fut> SubjectSource<S> for ResolvedSubject<F>
where
    S: Send + Sync + 'static,
    F: for<'a> Fn(&'a HttpContext<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<String>, AnyError>> + Send + 'static,
{
    fn subject_id<'a>(
        &'a self,
        ctx: &'a HttpContext<S>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, AnyError>> + Send + 'a>> {
        Box::pin((self.resolve)(ctx))
    }

    fn describe(&self) -> String {
        self.described_as.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{PrincipalSubject, ResolvedSubject, SubjectSource};
    use crate::AuthzError;
    use fundamental_auth::{AuthenticatedPrincipal, UserId};
    use fundamental_core::AnyError;
    use fundamental_server::HttpContext;

    #[derive(Clone)]
    struct State;

    fn anonymous() -> HttpContext<State> {
        HttpContext::new(State, "test")
    }

    fn signed_in() -> HttpContext<State> {
        HttpContext::new(State, "test").with_principal(Some(AuthenticatedPrincipal {
            user_id: UserId::new("alice"),
            device_id: None,
            scopes: Vec::new(),
            expires_at: 0,
        }))
    }

    #[tokio::test]
    async fn the_default_is_the_principal_verbatim() {
        let resolved = PrincipalSubject
            .subject_id(&signed_in())
            .await
            .expect("no store to fail");
        assert_eq!(resolved.as_deref(), Some("alice"));
    }

    /// An anonymous caller resolves to nobody rather than to an error.
    #[tokio::test]
    async fn an_anonymous_caller_resolves_to_nobody_not_to_a_failure() {
        let resolved = PrincipalSubject
            .subject_id(&anonymous())
            .await
            .expect("an anonymous caller is not a failure");
        assert_eq!(resolved, None);
    }

    /// A resolver that cannot answer surfaces an error, never "nobody".
    #[tokio::test]
    async fn a_resolver_that_cannot_answer_is_an_error_and_not_a_denial() {
        let unreachable = ResolvedSubject {
            resolve: |_: &HttpContext<State>| async {
                Err::<Option<String>, AnyError>(AnyError::new(AuthzError::UndefinedNamespace {
                    namespace: "the store did not answer".to_owned(),
                }))
            },
            described_as: "an unreachable store",
        };
        let failure = SubjectSource::subject_id(&unreachable, &signed_in()).await;
        assert!(
            failure.is_err(),
            "a resolver's failure collapsed into a denial"
        );
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_resolver_may_answer_with_a_subject_the_caller_is_not() {
        let membership = ResolvedSubject {
            resolve: |ctx: &HttpContext<State>| {
                let held = ctx
                    .principal_opt()
                    .map(|principal| format!("membership-of-{}", principal.user_id.as_str()));
                async move { Ok(held) }
            },
            described_as: "the caller's membership",
        };
        let resolved = SubjectSource::subject_id(&membership, &signed_in())
            .await
            .expect("no store to fail");
        assert_eq!(resolved.as_deref(), Some("membership-of-alice"));
    }
}
