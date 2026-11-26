//! [`OfflineTupleStore`]: the on-device (SQLite) relation-tuple store.

use fundamental_core::model::Offline;
use fundamental_db::{Filter, KeyedRepository, QueryBuilder, QueryExecutor, Repository};
use fundamental_offline::SharedOfflineDatabase;
use fundamental_offline::db::Value;
use fundamental_offline::store::OfflineStore;
use tokio::task::spawn_blocking;
use tracing::instrument;

use super::offline_row::OfflineRow;
use crate::backend::columns;
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use std::sync::Arc;

use crate::tuple::{
    ClockScope, DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef, TupleStore,
};
use crate::zookie::{BackendTag, Consistency, HybridClock, HybridTs};

/// Forward-read index over `(object_ns, object_id, relation)`.
fn forward_index_sql() -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS authz_rt_forward_idx ON authz_relation_tuple ({}, {}, {})",
        columns::OBJECT_NS,
        columns::OBJECT_ID,
        columns::RELATION,
    )
}

/// Reverse-read index over the subject columns.
fn reverse_index_sql() -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS authz_rt_reverse_idx ON authz_relation_tuple ({}, {}, {}, {}, {})",
        columns::SUBJECT_NS,
        columns::SUBJECT_ID,
        columns::SUBJECT_RELATION,
        columns::OBJECT_NS,
        columns::RELATION,
    )
}

/// A relation-tuple store over the on-device offline SQLite database.
///
/// The optimistic, on-device tier; the authoritative check is server-side. Uses the
/// framework's [`Repository`], [`QueryExecutor`] and [`QueryBuilder`]. Native targets only.
#[derive(Clone)]
pub struct OfflineTupleStore {
    db: SharedOfflineDatabase,
}

impl OfflineTupleStore {
    /// Wraps a shared offline database as a tuple store.
    #[must_use]
    pub fn new(db: SharedOfflineDatabase) -> Self {
        Self { db }
    }

    /// Creates the tuple table and its forward/reverse indexes if absent.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::SchemaSetup`] if the DDL fails, or
    /// [`AuthzError::StorageDecode`] if the blocking task cannot run.
    #[instrument(skip(self))]
    pub async fn ensure_schema(&self) -> Result<(), AuthzError> {
        let handle = self.db.inner();
        spawn_blocking(move || -> Result<(), AuthzError> {
            let db = handle
                .lock()
                .map_err(|_| AuthzError::StorageDecode("offline database lock poisoned".into()))?;
            OfflineStore::new(&db)
                .ensure_schema::<OfflineRow>()
                .map_err(|e| AuthzError::SchemaSetup(e.to_string()))?;
            for sql in [forward_index_sql(), reverse_index_sql()] {
                db.execute(&sql, &[])
                    .map_err(|e| AuthzError::SchemaSetup(e.to_string()))?;
            }
            Ok(())
        })
        .await
        .map_err(|e| AuthzError::StorageDecode(format!("schema task failed: {e}")))?
    }

    /// Runs a filtered query through the offline query executor and rebuilds tuples.
    async fn query(
        &self,
        builder: QueryBuilder<OfflineRow, Offline>,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let rows = self
            .db
            .query_executor::<OfflineRow>()
            .execute(&builder)
            .await?;
        rows.into_iter().map(OfflineRow::into_tuple).collect()
    }
}

impl TupleStore for OfflineTupleStore {
    fn backend(&self) -> BackendTag {
        BackendTag::Offline
    }

    type Scope = ClockScope;

    /// Runs `body` in a scope carrying the current instant.
    ///
    /// One local database already reflects every local write; this mirror is not the security
    /// boundary.
    async fn evaluate<'a, F, T>(
        &'a self,
        consistency: Consistency,
        body: F,
    ) -> Result<T, AuthzError>
    where
        F: Fn(Arc<Self::Scope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a,
    {
        let _ = consistency;
        body(Arc::new(ClockScope::current())).await
    }

    #[instrument(skip(self, _scope), fields(object = %object, relation = %relation, subject = %subject))]
    async fn read_edge(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        let key = RelationTuple::new(object.clone(), relation.clone(), subject.clone())
            .key()
            .as_str()
            .to_string();
        match self
            .db
            .repository::<OfflineRow>()
            .find(&Value::from(key))
            .await?
        {
            Some(row) => Ok(Some(DirectEdge::from_caveat(row.into_tuple()?.caveat))),
            None => Ok(None),
        }
    }

    #[instrument(skip(self, _scope), fields(object = %object, relation = %relation, limit))]
    async fn read_usersets(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let builder = QueryBuilder::<OfflineRow, Offline>::new()
            .filter(Filter::eq(columns::OBJECT_NS, object.namespace.as_str()))
            .filter(Filter::eq(columns::OBJECT_ID, object.id.as_str()))
            .filter(Filter::eq(columns::RELATION, relation.as_str()))
            .filter(Filter::ne(columns::SUBJECT_RELATION, ""))
            .limit(limit as u64);
        self.query(builder).await
    }

    #[instrument(skip(self, _scope), fields(object = %object, relation = %relation, limit))]
    async fn read_all(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let builder = QueryBuilder::<OfflineRow, Offline>::new()
            .filter(Filter::eq(columns::OBJECT_NS, object.namespace.as_str()))
            .filter(Filter::eq(columns::OBJECT_ID, object.id.as_str()))
            .filter(Filter::eq(columns::RELATION, relation.as_str()))
            .limit(limit as u64);
        self.query(builder).await
    }

    #[instrument(skip(self, _scope), fields(subject = %subject, limit))]
    async fn read_by_subject(
        &self,
        _scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let subject_relation = subject
            .relation
            .as_ref()
            .map(RelationName::as_str)
            .unwrap_or("");
        let mut builder = QueryBuilder::<OfflineRow, Offline>::new()
            .filter(Filter::eq(
                columns::SUBJECT_NS,
                subject.object.namespace.as_str(),
            ))
            .filter(Filter::eq(columns::SUBJECT_ID, subject.object.id.as_str()))
            .filter(Filter::eq(columns::SUBJECT_RELATION, subject_relation));
        if let Some(namespace) = object_namespace {
            builder = builder.filter(Filter::eq(columns::OBJECT_NS, namespace.as_str()));
        }
        if let Some(relation) = relation {
            builder = builder.filter(Filter::eq(columns::RELATION, relation.as_str()));
        }
        self.query(builder.limit(limit as u64)).await
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let repo = self.db.repository::<OfflineRow>();
        for tuple in tuples {
            repo.upsert(&OfflineRow::from_tuple(tuple)?).await?;
        }
        Ok(HybridClock::now())
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let repo = self.db.repository::<OfflineRow>();
        for tuple in tuples {
            let key = tuple.key().as_str().to_string();
            repo.delete(&Value::from(key)).await?;
        }
        Ok(HybridClock::now())
    }
}
