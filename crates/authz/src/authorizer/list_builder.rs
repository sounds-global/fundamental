//! [`ListBuilder`]: the awaitable builder a fluent `list` returns.

use std::future::{Future, IntoFuture};
use std::pin::Pin;

use super::DynAuthorizer;
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use crate::tuple::{CaveatContext, ObjectRef, SubjectRef};
use crate::zookie::{Consistency, Zookie};

/// A reverse walk is always bounded, so the fluent path picks a default page.
const DEFAULT_LIST_LIMIT: usize = 1000;

/// A prepared reverse lookup, awaited directly or after tuning its bound and freshness;
/// awaiting yields the matching `Vec<ObjectRef>`.
#[must_use = "a ListBuilder does nothing until awaited"]
pub struct ListBuilder<'a, T: ?Sized> {
    authz: &'a T,
    prepared: Result<(SubjectRef, NamespaceName, RelationName), AuthzError>,
    context: CaveatContext,
    consistency: Consistency,
    limit: usize,
}

impl<'a, T: DynAuthorizer + ?Sized> ListBuilder<'a, T> {
    pub(super) fn new(
        authz: &'a T,
        prepared: Result<(SubjectRef, NamespaceName, RelationName), AuthzError>,
    ) -> Self {
        Self {
            authz,
            prepared,
            context: CaveatContext::new(),
            consistency: Consistency::default(),
            limit: DEFAULT_LIST_LIMIT,
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

    /// Caps the number of objects returned (default `1000`).
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

impl<'a, T: DynAuthorizer + ?Sized> IntoFuture for ListBuilder<'a, T> {
    type Output = Result<Vec<ObjectRef>, AuthzError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let (subject, namespace, relation) = self.prepared?;
            let response = self
                .authz
                .list_objects(
                    subject,
                    namespace,
                    relation,
                    self.context,
                    self.consistency,
                    self.limit,
                )
                .await?;
            Ok(response.objects)
        })
    }
}
