//! The [`Authorizer`] trait: the framework's permission API.

use std::future::Future;

use super::{CheckResponse, ExpandResponse, ListObjectsResponse};
use crate::error::AuthzError;
use crate::schema::{Namespace, NamespaceName, Relation, RelationName};
use crate::tuple::{CanonicalId, CaveatContext, ObjectRef, RelationTuple, SubjectRef};
use crate::zookie::{Consistency, Zookie};

/// The framework's authorization API: ask permission questions and manage the
/// relationships that answer them.
///
/// [`NativeEngine`](super::NativeEngine) implements it locally. Reads return the [`Zookie`]
/// they were evaluated at and writes the one they committed at. For an object-safe handle,
/// see [`DynAuthorizer`](super::DynAuthorizer).
pub trait Authorizer: Send + Sync {
    /// Returns whether `subject` may perform `relation` on `object`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn check(
        &self,
        object: ObjectRef,
        relation: RelationName,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> impl Future<Output = Result<CheckResponse, AuthzError>> + Send;

    /// Expands `object#relation` into the tree of who holds it.
    fn expand(
        &self,
        object: ObjectRef,
        relation: RelationName,
        consistency: Consistency,
    ) -> impl Future<Output = Result<ExpandResponse, AuthzError>> + Send;

    /// Lists objects of `object_namespace` on which `subject` holds `relation`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn list_objects(
        &self,
        subject: SubjectRef,
        object_namespace: NamespaceName,
        relation: RelationName,
        context: CaveatContext,
        consistency: Consistency,
        limit: usize,
    ) -> impl Future<Output = Result<ListObjectsResponse, AuthzError>> + Send;

    /// Writes (upserts) relationship tuples, returning the commit zookie.
    fn write(
        &self,
        tuples: &[RelationTuple],
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send;

    /// Deletes relationship tuples by identity, returning the commit zookie.
    fn delete(
        &self,
        tuples: &[RelationTuple],
    ) -> impl Future<Output = Result<Zookie, AuthzError>> + Send;

    /// Typed check: the relation is a variant of the object's namespace enum, so a
    /// `Document::Viewer` check cannot be issued against a `Folder`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn check_typed<N>(
        &self,
        relation: N::Relation,
        object: CanonicalId,
        subject: SubjectRef,
        context: CaveatContext,
        consistency: Consistency,
    ) -> impl Future<Output = Result<CheckResponse, AuthzError>> + Send
    where
        N: Namespace,
    {
        let namespace = NamespaceName::new(N::NAME);
        let relation = RelationName::new(relation.relation_name());
        // Through the namespace's canonical form, as the permission guard does.
        let canonical = N::canonical_id(object.as_str())
            .map(|id| CanonicalId::new(id.as_ref()))
            .transpose()
            .and_then(|id| {
                id.ok_or_else(|| AuthzError::InvalidCanonicalId {
                    value: object.as_str().chars().take(64).collect(),
                })
            });
        async move {
            let object = ObjectRef::new(namespace?, canonical?);
            self.check(object, relation?, subject, context, consistency)
                .await
        }
    }
}
