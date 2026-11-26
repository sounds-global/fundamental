//! [`PermissionGuard`]: the `authorize(...)` HTTP permission middleware.

use fundamental_core::AnyError;
use fundamental_server::{HttpContext, Middleware, MiddlewareResult, Next};
use tracing::error;

use super::forbidden::Forbidden;
use super::guard_check::GuardCheck;
use super::guard_decision::GuardDecision;
use super::object_source::{ConstObject, ObjectSource, PathObject, StateObject};
use super::subject_source::{PrincipalSubject, ResolvedSubject, SubjectSource};
use crate::authorizer::HasAuthorizer;
use crate::schema::{Namespace, Relation};
use crate::zookie::Consistency;

/// A permission guard: middleware that lets a request proceed only if the caller
/// holds a relation on the object the route is about.
///
/// Attach it to a route with [`authorize`]:
///
/// ```no_run
/// # extern crate fundamental_authz;
/// # use fundamental_authz::middleware::authorize;
/// # enum Document { Viewer }
/// # // (Document derives Namespace in real code.)
/// # fn _demo() {
/// // #[action(GET, "/documents/{id}", auth, authorize(Document::Viewer))]
/// // — the object id defaults to the sole `{id}` path parameter.
/// # }
/// ```
///
/// It resolves the object through an [`ObjectSource`], takes the subject from the request's
/// principal, and asks the [`Authorizer`](crate::authorizer::Authorizer) reached through
/// [`HasAuthorizer`].
///
/// An allow continues; a denial is `403` ([`Forbidden::Denied`]), or `401` with no principal;
/// a backend failure surfaces as itself, never as a denial. A request with no principal is
/// checked as the public wildcard, so [`Middleware::NEEDS_PRINCIPAL`] stays `false`. Caveats
/// read the scope's own `now`, never a request value. Defaults to
/// [`Consistency::FullConsistency`], against the "new enemy" problem.
#[must_use]
pub struct PermissionGuard<O = PathObject, Sb = PrincipalSubject> {
    check: GuardCheck,
    object: O,
    /// Who the request is authorized as; by default the principal verbatim.
    subject: Sb,
}

/// Builds a permission guard requiring `relation` on the object type `N`.
///
/// `N` is inferred from the relation value; the object id is the sole path parameter unless
/// [`object`](PermissionGuard::object), [`object_const`](PermissionGuard::object_const) or
/// [`object_from_state`](PermissionGuard::object_from_state) says otherwise.
pub fn authorize<N>(relation: N) -> PermissionGuard<PathObject>
where
    N: Namespace + Relation,
{
    PermissionGuard {
        check: GuardCheck::of(relation),
        object: PathObject::default(),
        subject: PrincipalSubject,
    }
}

impl<O, Sb> PermissionGuard<O, Sb> {
    /// The namespace of the objects this guard authorizes.
    #[must_use]
    pub fn object_namespace(&self) -> &str {
        self.check.object_namespace()
    }
}

impl<Sb> PermissionGuard<PathObject, Sb> {
    /// Names the path parameter holding the object id, needed only with several.
    pub fn object(mut self, param: impl Into<String>) -> Self {
        self.object = PathObject {
            param: Some(param.into()),
        };
        self
    }

    /// Fixes the object id for every request to this route.
    pub fn object_const(self, id: impl Into<String>) -> PermissionGuard<ConstObject, Sb> {
        self.with_object(ConstObject(id.into()))
    }

    /// Derives the object id from the app state, which a route declaration cannot know.
    pub fn object_from_state<S, F>(self, source: F) -> PermissionGuard<StateObject<F>, Sb>
    where
        F: Fn(&S) -> Option<String> + Send + Sync + 'static,
    {
        self.with_object(StateObject(source))
    }
}

impl<O, Sb> PermissionGuard<O, Sb> {
    /// Overrides the namespace the principal is treated as a subject of (default `user`).
    pub fn subject_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.check = self.check.with_subject_namespace(namespace);
        self
    }

    /// Overrides the freshness this check reads at (default
    /// [`Consistency::FullConsistency`]).
    pub fn consistency(mut self, consistency: Consistency) -> Self {
        self.check = self.check.with_consistency(consistency);
        self
    }

    /// Resolves the subject from the request rather than taking the principal verbatim.
    ///
    /// The closure may await, so the subject can be a row (a membership, a delegation) rather than
    /// the credential. `Ok(None)` denies as the public wildcard; `Err` surfaces as itself, never
    /// as a denial.
    pub fn subject_from<S, F, Fut>(
        self,
        described_as: &'static str,
        resolve: F,
    ) -> PermissionGuard<O, ResolvedSubject<F>>
    where
        F: for<'a> Fn(&'a HttpContext<S>) -> Fut + Send + Sync + 'static,
        Fut: core::future::Future<Output = Result<Option<String>, AnyError>> + Send + 'static,
    {
        self.with_subject(ResolvedSubject {
            resolve,
            described_as,
        })
    }

    /// Rebuilds the guard around a different object source.
    fn with_object<P>(self, object: P) -> PermissionGuard<P, Sb> {
        PermissionGuard {
            check: self.check,
            object,
            subject: self.subject,
        }
    }

    /// Rebuilds the guard around a different subject source.
    fn with_subject<P>(self, subject: P) -> PermissionGuard<O, P> {
        PermissionGuard {
            check: self.check,
            object: self.object,
            subject,
        }
    }
}

impl<S, O, Sb> Middleware<S> for PermissionGuard<O, Sb>
where
    S: HasAuthorizer + Send + Sync + 'static,
    O: ObjectSource<S>,
    Sb: SubjectSource<S>,
{
    // `NEEDS_PRINCIPAL` stays `false`: an anonymous request is checked as the public wildcard.
    const GRANTS_AUTHORIZATION: bool = true;

    // `PathObject` reads the path; the other sources do not.
    const AUTHORIZES_FROM_PATH: bool = <O as ObjectSource<S>>::FROM_PATH;

    async fn handle(
        &self,
        ctx: HttpContext<S>,
        body: Vec<u8>,
        next: Next<'_, S>,
    ) -> MiddlewareResult {
        // A subject lookup that fails surfaces as its own error, never as a denial.
        let principal = self.subject.subject_id(&ctx).await.map_err(|error| {
            error!(
                object_namespace = %self.check.object_namespace(),
                relation = %self.check.relation(),
                source = %self.subject.describe(),
                error = %error,
                "authorization guard could not resolve a subject; refusing rather than denying"
            );
            error
        })?;

        let object_id = self.object.object_id(&ctx).ok_or_else(|| {
            error!(
                object_namespace = %self.check.object_namespace(),
                relation = %self.check.relation(),
                source = %self.object.describe(),
                "authorization guard resolved no object id"
            );
            AnyError::new(Forbidden::MissingObject)
        })?;

        let decision = self
            .check
            .decide(ctx.state().authorizer(), principal.as_deref(), &object_id)
            .await?;
        match decision {
            GuardDecision::Allowed { object_id } => {
                ctx.authorized_objects().record(
                    self.check.object_namespace(),
                    self.check.relation(),
                    &object_id,
                );
                next.run(ctx, body).await
            }
            denied => Err(denied
                .refusal()
                .unwrap_or_else(|| AnyError::new(Forbidden::Denied))),
        }
    }
}
