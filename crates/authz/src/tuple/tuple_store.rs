//! [`TupleStore`]: the backend-neutral seam the check engine reads and writes through.

use std::future::Future;

use std::sync::Arc;

use super::{DirectEdge, ObjectRef, ReadScope, RelationTuple, ScopedEval, SubjectRef};
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use crate::zookie::{BackendTag, Consistency, HybridTs};

/// The storage seam the engine evaluates over, shaped around a Zanzibar check's access
/// patterns.
///
/// Concrete subjects and wildcards are point lookups ([`read_edge`](TupleStore::read_edge));
/// only usersets are enumerated, each bounded by a `limit`. Every read of one evaluation goes
/// through one [`ReadScope`]; a write returns the [`HybridTs`] it committed at.
///
/// # Errors
///
/// A backend failure is [`AuthzError::Storage`], never an absence that reads as a denial.
pub trait TupleStore: Send + Sync {
    /// This store's read handle. See [`ReadScope`].
    type Scope: ReadScope + Send + Sync;

    /// The backend this store is, so the engine can tag zookies with their origin.
    fn backend(&self) -> BackendTag;

    /// Opens a read scope honouring `consistency`, runs `body` inside it, and closes
    ///
    /// One method rather than an `open`/`close` pair, so the scope cannot outlive the call.
    /// `body` is [`AsyncFn`] because a backend may run it again at a fresh read point.
    ///
    /// # Errors
    ///
    /// Returns whatever `body` returns, or [`AuthzError::Storage`] if the scope
    /// cannot be opened or closed.
    fn evaluate<'a, F, T>(
        &'a self,
        consistency: Consistency,
        body: F,
    ) -> impl Future<Output = Result<T, AuthzError>> + Send + 'a
    where
        F: Fn(Arc<Self::Scope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a;

    /// Point-looks-up one exact edge `object#relation@subject` within `scope`.
    ///
    /// `None` when the edge is absent.
    fn read_edge(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> impl Future<Output = Result<Option<DirectEdge>, AuthzError>> + Send;

    /// Enumerates the userset edges stored on `object#relation`, up to `limit`, within `scope`;
    /// leaves and wildcards are for [`read_edge`](TupleStore::read_edge).
    fn read_usersets(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<RelationTuple>, AuthzError>> + Send;

    /// Enumerates all edges stored on `object#relation`, up to `limit`, within `snapshot`: what a
    /// `tuple_to_userset` hop reads.
    fn read_all(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<RelationTuple>, AuthzError>> + Send;

    /// Reverse read within `scope`: the tuples in which `subject` appears directly, optionally
    /// narrowed to a namespace and relation, up to `limit`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn read_by_subject(
        &self,
        scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<RelationTuple>, AuthzError>> + Send;

    /// Upserts tuples, returning the timestamp they committed at.
    fn write(
        &self,
        tuples: &[RelationTuple],
    ) -> impl Future<Output = Result<HybridTs, AuthzError>> + Send;

    /// Deletes tuples by identity, returning the timestamp the deletion committed at.
    fn delete(
        &self,
        tuples: &[RelationTuple],
    ) -> impl Future<Output = Result<HybridTs, AuthzError>> + Send;
}
