//! [`DynAuthorizer`]: an object-safe view of an [`Authorizer`].

use std::future::Future;
use std::pin::Pin;

use super::{Authorizer, CheckResponse, ExpandResponse, ListObjectsResponse};
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use crate::tuple::{CaveatContext, ObjectRef, RelationTuple, SubjectRef};
use crate::zookie::{Consistency, Zookie};

type CheckFuture<'a> = Pin<Box<dyn Future<Output = Result<CheckResponse, AuthzError>> + Send + 'a>>;
type ExpandFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ExpandResponse, AuthzError>> + Send + 'a>>;
type ListFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ListObjectsResponse, AuthzError>> + Send + 'a>>;
type ZookieFuture<'a> = Pin<Box<dyn Future<Output = Result<Zookie, AuthzError>> + Send + 'a>>;

/// An object-safe authorizer, for application state (`Arc<dyn DynAuthorizer>`).
///
/// Boxes the futures [`Authorizer`](super::Authorizer) returns; a blanket impl adapts every
/// `Authorizer`. `check_typed` stays on [`Authorizer`](super::Authorizer).
pub trait DynAuthorizer: Send + Sync {
    /// See [`Authorizer::check`](super::Authorizer::check).
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn check<'a>(
        &'a self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> CheckFuture<'a>;

    /// See [`Authorizer::expand`](super::Authorizer::expand).
    fn expand<'a>(
        &'a self,
        object: ObjectRef,
        relation: RelationName,
        consistency: Consistency,
    ) -> ExpandFuture<'a>;

    /// See [`Authorizer::list_objects`](super::Authorizer::list_objects).
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn list_objects<'a>(
        &'a self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        consistency: Consistency,
        limit: usize,
    ) -> ListFuture<'a>;

    /// See [`Authorizer::write`](super::Authorizer::write).
    fn write<'a>(&'a self, tuples: &'a [RelationTuple]) -> ZookieFuture<'a>;

    /// See [`Authorizer::delete`](super::Authorizer::delete).
    fn delete<'a>(&'a self, tuples: &'a [RelationTuple]) -> ZookieFuture<'a>;
}

impl<A: Authorizer> DynAuthorizer for A {
    fn check<'a>(
        &'a self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> CheckFuture<'a> {
        Box::pin(Authorizer::check(
            self,
            object,
            relation,
            subject,
            context,
            consistency,
        ))
    }

    fn expand<'a>(
        &'a self,
        object: ObjectRef,
        relation: RelationName,
        consistency: Consistency,
    ) -> ExpandFuture<'a> {
        Box::pin(Authorizer::expand(self, object, relation, consistency))
    }

    fn list_objects<'a>(
        &'a self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        consistency: Consistency,
        limit: usize,
    ) -> ListFuture<'a> {
        Box::pin(Authorizer::list_objects(
            self,
            subject,
            object_namespace,
            relation,
            context,
            consistency,
            limit,
        ))
    }

    fn write<'a>(&'a self, tuples: &'a [RelationTuple]) -> ZookieFuture<'a> {
        Box::pin(Authorizer::write(self, tuples))
    }

    fn delete<'a>(&'a self, tuples: &'a [RelationTuple]) -> ZookieFuture<'a> {
        Box::pin(Authorizer::delete(self, tuples))
    }
}
