//! [`SqlTransactionView`]: the SQL tuple store, read from inside a caller's transaction.

use std::sync::Arc;

use fundamental_db::DatabaseError;
use fundamental_sql::{ClusterTime, SqlTransaction};

use super::sql_transaction_scope::SqlTransactionScope;
use super::sql_tuple_store::SqlTupleStore;
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use crate::tuple::{
    DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef, TupleStore, WallTime,
};
use crate::zookie::{BackendTag, Consistency, HybridTs};

/// The relation-tuple store as seen through a transaction somebody else opened.
///
/// The same engine walks it, with the same queries as [`SqlTupleStore`], but every read goes
/// through the caller's transaction. Built per decision by
/// [`TransactionalAuthorizer`](crate::authorizer::TransactionalAuthorizer). It never writes:
/// the caller writes with [`SqlTupleStore::write_in`] and
/// [`delete_in`](SqlTupleStore::delete_in).
pub(crate) struct SqlTransactionView<'t> {
    scope: Arc<SqlTransactionScope<'t>>,
}

impl<'t> SqlTransactionView<'t> {
    /// A view reading through `tx`, its caveats judged at the cluster's clock read now.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::Storage`] if the clock cannot be read.
    pub(crate) async fn open(tx: &'t mut SqlTransaction) -> Result<Self, AuthzError> {
        let clock = ClusterTime::read_in(tx)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        let now = WallTime::from_unix_nanos(clock.nanos());
        Ok(Self {
            scope: Arc::new(SqlTransactionScope::new(tx, now)),
        })
    }

    pub(crate) fn scope(&self) -> &SqlTransactionScope<'t> {
        &self.scope
    }
}

impl<'t> TupleStore for SqlTransactionView<'t> {
    type Scope = SqlTransactionScope<'t>;

    fn backend(&self) -> BackendTag {
        BackendTag::SqlDb
    }

    async fn evaluate<'a, F, T>(
        &'a self,
        consistency: Consistency,
        body: F,
    ) -> Result<T, AuthzError>
    where
        F: Fn(Arc<Self::Scope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a,
    {
        // The caller's transaction fixes what is read; a restart belongs to its whole transaction.
        let _ = consistency;
        body(Arc::clone(&self.scope)).await
    }

    async fn read_edge(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        let tuples = scope
            .query(SqlTupleStore::edge_query(object, relation, subject))
            .await?;
        Ok(SqlTupleStore::edge_of(tuples))
    }

    async fn read_usersets(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        scope
            .query(SqlTupleStore::usersets_query(object, relation, limit))
            .await
    }

    async fn read_all(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        scope
            .query(SqlTupleStore::all_query(object, relation, limit))
            .await
    }

    async fn read_by_subject(
        &self,
        scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        scope
            .query(SqlTupleStore::by_subject_query(
                subject,
                object_namespace,
                relation,
                limit,
            ))
            .await
    }

    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let _ = tuples;
        Err(DatabaseError::query(
            "a decision inside a transaction does not write; use SqlTupleStore::write_in",
        )
        .into())
    }

    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let _ = tuples;
        Err(DatabaseError::query(
            "a decision inside a transaction does not delete; use SqlTupleStore::delete_in",
        )
        .into())
    }
}
