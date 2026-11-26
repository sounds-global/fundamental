//! [`CheckBuilder`]: the awaitable builder a fluent `check` returns.

use std::future::{Future, IntoFuture};
use std::pin::Pin;

use super::{CheckResponse, DynAuthorizer};
use crate::error::AuthzError;
use crate::schema::RelationName;
use crate::tuple::{CaveatContext, ObjectRef, SubjectRef};
use crate::zookie::{Consistency, Zookie};

/// A prepared permission check, awaited directly or after tuning its freshness.
///
/// Awaiting it yields the bare `bool`; [`response`](CheckBuilder::response) yields the full
/// [`CheckResponse`]:
///
/// ```text
/// authz.can(alice, Document::Viewer, "readme").await?;                       // -> bool
/// authz.can(alice, Document::Viewer, "readme").at_least_as_fresh(z).await?;  // -> bool, pinned
/// authz.can(alice, Document::Viewer, "readme").response().await?;            // -> CheckResponse
/// ```
///
/// An argument that failed to validate surfaces as the check's error when it runs.
#[must_use = "a CheckBuilder does nothing until awaited"]
pub struct CheckBuilder<'a, T: ?Sized> {
    authz: &'a T,
    prepared: Result<(ObjectRef, RelationName, SubjectRef), AuthzError>,
    context: CaveatContext,
    consistency: Consistency,
}

impl<'a, T: DynAuthorizer + ?Sized> CheckBuilder<'a, T> {
    pub(super) fn new(
        authz: &'a T,
        prepared: Result<(ObjectRef, RelationName, SubjectRef), AuthzError>,
    ) -> Self {
        Self {
            authz,
            prepared,
            context: CaveatContext::new(),
            consistency: Consistency::default(),
        }
    }

    /// Supplies a request caveat context, which must be empty or the check is refused with
    /// [`AuthzError::RequestCaveatParameter`](crate::AuthzError::RequestCaveatParameter).
    pub fn context(mut self, context: CaveatContext) -> Self {
        self.context = context;
        self
    }

    /// Sets the read consistency.
    pub fn consistency(mut self, consistency: Consistency) -> Self {
        self.consistency = consistency;
        self
    }

    /// Requires the read reflect state at least as fresh as `zookie`.
    pub fn at_least_as_fresh(mut self, zookie: Zookie) -> Self {
        self.consistency = Consistency::AtLeastAsFresh(zookie);
        self
    }

    /// Resolves to the full [`CheckResponse`], with the [`Zookie`] it was evaluated at.
    #[must_use = "the returned future does nothing until awaited"]
    pub fn response(
        self,
    ) -> Pin<Box<dyn Future<Output = Result<CheckResponse, AuthzError>> + Send + 'a>> {
        Box::pin(async move {
            let (object, relation, subject) = self.prepared?;
            self.authz
                .check(object, relation, subject, self.context, self.consistency)
                .await
        })
    }
}

impl<'a, T: DynAuthorizer + ?Sized> IntoFuture for CheckBuilder<'a, T> {
    type Output = Result<bool, AuthzError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        let response = self.response();
        Box::pin(async move { Ok(response.await?.allowed) })
    }
}
