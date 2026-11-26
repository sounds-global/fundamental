//! [`CqlTupleStore`]: the massive-scale relation-tuple store over YCQL.

use fundamental_cql::{CqlPool, CqlValue, Ddl, ToRow};
use fundamental_db::{KeyedRepository, RepositoryBatch};
use tracing::instrument;

use super::forward_row::{ForwardRow, SUBJECT_KIND_USERSET, subject_kind};
use super::reverse_row::ReverseRow;
use crate::backend::columns;
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use std::sync::Arc;

use crate::tuple::{
    ClockScope, DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef, TupleStore,
};
use crate::zookie::{BackendTag, Consistency, HybridClock, HybridTs};

/// A relation-tuple store over YCQL, for massive scale.
///
/// Shaped around partition-keyed reads: a forward table by `object#relation` and a reverse
/// table by subject, kept in step by dual writes since YCQL has no cross-partition atomic
/// write. Call [`ensure_schema`](CqlTupleStore::ensure_schema) once.
#[derive(Clone)]
pub struct CqlTupleStore {
    pool: CqlPool,
}

impl CqlTupleStore {
    /// Wraps a YCQL pool as a tuple store.
    #[must_use]
    pub fn new(pool: CqlPool) -> Self {
        Self { pool }
    }

    /// Creates the forward and reverse tables if absent.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::SchemaSetup`] if a `CREATE TABLE` fails.
    #[instrument(skip(self))]
    pub async fn ensure_schema(&self) -> Result<(), AuthzError> {
        for sql in [
            ForwardRow::create_table_sql(),
            ReverseRow::create_table_sql(),
        ] {
            // `Ddl::interpolated`: the text is the derive's generated `CREATE TABLE`.
            self.pool
                .execute_ddl(&Ddl::interpolated(sql.to_owned()))
                .await
                .map_err(|e| AuthzError::SchemaSetup(e.to_string()))?;
        }
        Ok(())
    }
}

impl TupleStore for CqlTupleStore {
    fn backend(&self) -> BackendTag {
        BackendTag::CqlDb
    }

    type Scope = ClockScope;

    /// Runs `body` in a scope carrying the current instant.
    ///
    /// YCQL has no cross-partition snapshot: reads give per-edge read-your-writes, not one
    /// coherent state.
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
        let subject_relation = subject
            .relation
            .as_ref()
            .map(RelationName::as_str)
            .unwrap_or("");
        let row = self
            .pool
            .query::<ForwardRow>()
            .partition_key_composite([
                CqlValue::text(object.namespace.as_str()),
                CqlValue::text(object.id.as_str()),
                CqlValue::text(relation.as_str()),
            ])
            .clustering_eq(columns::SUBJECT_KIND, CqlValue::text(subject_kind(subject)))
            .clustering_eq(
                columns::SUBJECT_NS,
                CqlValue::text(subject.object.namespace.as_str()),
            )
            .clustering_eq(
                columns::SUBJECT_ID,
                CqlValue::text(subject.object.id.as_str()),
            )
            .clustering_eq(columns::SUBJECT_RELATION, CqlValue::text(subject_relation))
            .execute_one()
            .await?;
        match row {
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
        let rows = self
            .pool
            .query::<ForwardRow>()
            .partition_key_composite([
                CqlValue::text(object.namespace.as_str()),
                CqlValue::text(object.id.as_str()),
                CqlValue::text(relation.as_str()),
            ])
            .clustering_eq(columns::SUBJECT_KIND, CqlValue::text(SUBJECT_KIND_USERSET))
            .limit(limit as u32)
            .execute()
            .await?;
        rows.into_iter().map(ForwardRow::into_tuple).collect()
    }

    #[instrument(skip(self, _scope), fields(object = %object, relation = %relation, limit))]
    async fn read_all(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let rows = self
            .pool
            .query::<ForwardRow>()
            .partition_key_composite([
                CqlValue::text(object.namespace.as_str()),
                CqlValue::text(object.id.as_str()),
                CqlValue::text(relation.as_str()),
            ])
            .limit(limit as u32)
            .execute()
            .await?;
        rows.into_iter().map(ForwardRow::into_tuple).collect()
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
        let mut query = self.pool.query::<ReverseRow>().partition_key_composite([
            CqlValue::text(subject.object.namespace.as_str()),
            CqlValue::text(subject.object.id.as_str()),
            CqlValue::text(subject_relation),
        ]);
        // Narrow by the clustering prefix (object_ns, then relation); a relation filter without an
        // object namespace is not a prefix, so it applies after the read.
        let mut relation_pushed_to_clustering = false;
        if let Some(namespace) = object_namespace {
            query = query.clustering_eq(columns::OBJECT_NS, CqlValue::text(namespace.as_str()));
            if let Some(relation) = relation {
                query = query.clustering_eq(columns::RELATION, CqlValue::text(relation.as_str()));
                relation_pushed_to_clustering = true;
            }
        }
        let rows = query.limit(limit as u32).execute().await?;
        let mut tuples: Vec<RelationTuple> = rows
            .into_iter()
            .map(ReverseRow::into_tuple)
            .collect::<Result<_, _>>()?;
        if let Some(relation) = relation
            && !relation_pushed_to_clustering
        {
            tuples.retain(|t| &t.relation == relation);
        }
        Ok(tuples)
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        if tuples.is_empty() {
            return Ok(HybridClock::now());
        }
        let forward: Vec<ForwardRow> = tuples
            .iter()
            .map(ForwardRow::from_tuple)
            .collect::<Result<_, _>>()?;
        let reverse: Vec<ReverseRow> = tuples
            .iter()
            .map(ReverseRow::from_tuple)
            .collect::<Result<_, _>>()?;
        // YCQL INSERT is an upsert; write forward, then the reverse view.
        self.pool
            .repository::<ForwardRow>()
            .insert_many(&forward)
            .await?;
        self.pool
            .repository::<ReverseRow>()
            .insert_many(&reverse)
            .await?;
        Ok(HybridClock::now())
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let forward_repo = self.pool.repository::<ForwardRow>();
        let reverse_repo = self.pool.repository::<ReverseRow>();
        for tuple in tuples {
            let forward = ForwardRow::from_tuple(tuple)?;
            let reverse = ReverseRow::from_tuple(tuple)?;
            forward_repo.delete(&forward.to_key_values()).await?;
            reverse_repo.delete(&reverse.to_key_values()).await?;
        }
        Ok(HybridClock::now())
    }
}
