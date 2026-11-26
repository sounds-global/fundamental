//! [`SqlTupleStore`]: the relation-tuple store backed by the SQL wire.

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{
    DatabaseError, Filter, IsolationLevel, KeyedRepositoryTransaction, QueryBuilder, Repository,
    RepositoryTransaction, RepositoryTransactionQuery, Transaction, TransactionOptions,
    Transactional,
};
use fundamental_sql::migration::TableCatalog;
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use tracing::{debug, instrument, warn};

use super::relation_tuple_row::RelationTupleRow;
use super::sql_read_scope::SqlReadScope;
use super::tuple_table_problems::tuple_table_problems;
use crate::backend::columns;
use crate::engine::Witness;
use crate::error::AuthzError;
use crate::schema::{NamespaceName, RelationName};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::tuple::{
    DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef, TupleKey, TupleStore,
};
use crate::zookie::{BackendTag, Consistency, HybridTs};

/// The model's own table, so the boot check judges the table the repository writes.
const TUPLE_TABLE: &str = <RelationTupleRow as SqlModel>::TABLE;

/// A relation-tuple store over the SQL wire.
///
/// Each evaluation reads in one read-only `REPEATABLE READ` transaction. The store issues no
/// DDL: the app's migrator creates the table from
/// [`SQL_TUPLE_MIGRATIONS`](crate::SQL_TUPLE_MIGRATIONS); call
/// [`verify_schema`](SqlTupleStore::verify_schema) once at boot.
#[derive(Clone)]
pub struct SqlTupleStore {
    pool: SqlPool,
}

impl SqlTupleStore {
    /// Wraps a SQL pool as a tuple store.
    #[must_use]
    pub fn new(pool: SqlPool) -> Self {
        Self { pool }
    }

    /// Confirms the tuple table is the one [`SQL_TUPLE_MIGRATIONS`](crate::SQL_TUPLE_MIGRATIONS)
    /// creates, and changes nothing.
    ///
    /// Needed because the set is `IF NOT EXISTS`: a table made otherwise would be recorded as
    /// applied and left as it was. It reads the engine's catalog ([`TableCatalog`]).
    ///
    /// # Errors
    ///
    /// [`AuthzError::TupleTableMissing`] when the table is not on the connection's search path;
    /// [`AuthzError::TupleTableMismatch`] naming every difference; [`AuthzError::Storage`] when
    /// the catalog cannot be read.
    #[instrument(skip(self), fields(table = TUPLE_TABLE))]
    pub async fn verify_schema(&self) -> Result<(), AuthzError> {
        let catalog = self.catalog().await?;
        let problems = tuple_table_problems(&catalog)?;
        if problems.is_empty() {
            debug!("the tuple table matches its migrations");
            return Ok(());
        }
        let problems = problems.join("; ");
        warn!(%problems, "the tuple table differs from its migrations");
        Err(AuthzError::TupleTableMismatch { problems })
    }

    /// Confirms the connected role can change the tuple table's rows and nothing else, for a
    /// deployment that runs its migrator as a separate role.
    ///
    /// # Errors
    ///
    /// [`AuthzError::TupleRoleTooBroad`] naming every excess privilege;
    /// [`AuthzError::TupleTableMissing`] when there is no table to judge;
    /// [`AuthzError::Storage`] when the catalog cannot be read.
    #[instrument(skip(self), fields(table = TUPLE_TABLE))]
    pub async fn verify_runtime_role(&self) -> Result<(), AuthzError> {
        let excess = self.catalog().await?.access.excess();
        if excess.is_empty() {
            debug!("the runtime role holds only row access to the tuple table");
            return Ok(());
        }
        let excess = excess.join(", ");
        warn!(%excess, "the runtime role can alter the tuple table");
        Err(AuthzError::TupleRoleTooBroad { excess })
    }

    /// The tuple table as the catalog describes it, read again when another session's DDL moved
    /// the catalog under the read — a conflict, not an answer about this table.
    async fn catalog(&self) -> Result<TableCatalog, AuthzError> {
        self.pool
            .with_retry(|| TableCatalog::read(&self.pool, TUPLE_TABLE))
            .await
            .map_err(DatabaseError::from)?
            .ok_or_else(|| {
                warn!("the tuple table is not on the search path");
                AuthzError::TupleTableMissing
            })
    }

    /// Writes tuples inside a caller's transaction, so a resource and the
    /// authorization over it commit together or not at all.
    ///
    /// [`TupleStore::write`] commits on the store's own pool; this joins the caller's commit.
    /// Takes a [`SqlTransaction`] so the row type stays private.
    ///
    /// # Returns no zookie
    /// The write has no commit timestamp until the caller commits.
    ///
    /// # Locks in key order
    ///
    /// One statement per row, in ascending tuple-key order, the order a locked check locks in;
    /// a tuple given twice is written once, as given last.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError`] if a tuple cannot be encoded or the upsert fails; the caller rolls
    /// back its transaction.
    #[instrument(skip(tx, tuples), fields(tuples = tuples.len()))]
    pub async fn write_in(
        tx: &mut SqlTransaction,
        tuples: &[RelationTuple],
    ) -> Result<(), AuthzError> {
        for tuple in in_key_order(tuples) {
            // The same conversion as `write`, so the tuple key cannot differ inside a transaction.
            tx.upsert(&RelationTupleRow::from_tuple(tuple)?).await?;
        }
        Ok(())
    }

    /// Removes tuples inside a caller's transaction, so a resource and the authority
    /// over it stop existing together or not at all.
    ///
    /// [`TupleStore::delete`] commits on the store's own pool, which would leave a row and its
    /// authority to disappear in two commits. A tuple already gone is not an error, so a retried
    /// erasure succeeds; the count actually removed is returned.
    ///
    /// # Returns no zookie
    /// As [`write_in`](Self::write_in).
    ///
    /// # Locks in key order
    ///
    /// As [`write_in`](Self::write_in).
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError`] if the delete fails; the caller rolls back its transaction.
    #[instrument(skip(tx, tuples), fields(tuples = tuples.len(), removed))]
    pub async fn delete_in(
        tx: &mut SqlTransaction,
        tuples: &[RelationTuple],
    ) -> Result<usize, AuthzError> {
        let mut removed = 0_usize;
        for tuple in in_key_order(tuples) {
            // The tuple's own key, as `write_in` derives it, so a revocation undoes the same row.
            if KeyedRepositoryTransaction::<RelationTupleRow, SqlDb>::delete(
                tx,
                &tuple.key().as_str().to_string(),
            )
            .await?
            {
                removed += 1;
            }
        }
        tracing::Span::current().record("removed", removed);
        Ok(removed)
    }

    /// Locks every tuple of `witness` `FOR UPDATE` in the caller's transaction, one point
    /// statement per key in ascending key order, and returns each row as the lock found it —
    /// `None` where it is gone.
    ///
    /// One statement per key, because a multi-row `FOR UPDATE` on a hash-sharded table locks in
    /// no stated order; this order everywhere keeps tuple locks from deadlocking.
    ///
    /// # Errors
    ///
    /// [`AuthzError::Storage`] if a lock cannot be taken; [`AuthzError::StorageDecode`] if a
    /// locked row does not decode.
    #[instrument(skip_all, fields(witness = witness.len()))]
    pub(crate) async fn lock_in(
        tx: &mut SqlTransaction,
        witness: &Witness,
    ) -> Result<Vec<Option<RelationTuple>>, AuthzError> {
        let mut locked = Vec::with_capacity(witness.len());
        for tuple in witness.tuples() {
            let key = tuple.key().as_str().to_owned();
            let row = tx.find_for_update::<RelationTupleRow>(&key).await?;
            locked.push(row.map(RelationTupleRow::into_tuple).transpose()?);
        }
        Ok(locked)
    }

    /// The isolation the store's own revocation runs at.
    const REVOCATION_ISOLATION: IsolationLevel = IsolationLevel::ReadCommitted;

    /// The isolation an evaluation reads at.
    ///
    /// `REPEATABLE READ` observes one MVCC state; `READ COMMITTED` would let reads straddle a write.
    const EVALUATION_ISOLATION: IsolationLevel = IsolationLevel::RepeatableRead;

    /// How many times an evaluation is re-run at a fresh read point before failing.
    const READ_RESTARTS: u32 = 3;

    async fn attempt<'a, F, T>(&'a self, body: &F) -> Result<T, AuthzError>
    where
        F: Fn(Arc<SqlReadScope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a,
    {
        // Read-only, enforced by the server: an evaluation must never write.
        let mut tx = self
            .pool
            .begin_with(TransactionOptions::new(Self::EVALUATION_ISOLATION).read_only())
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        // Read before the snapshot is established, so the zookie understates freshness: the safe
        // direction.
        let ts = {
            let ct = ClusterTime::read_in(&mut tx)
                .await
                .map_err(|e| DatabaseError::query(e.to_string()))?;
            HybridTs::new(ct.nanos(), u32::from(ct.logical()))
        };

        let scope = Arc::new(SqlReadScope::new(tx, ts));
        let outcome = body(Arc::clone(&scope)).await;
        // The only handle left, so the transaction can be reclaimed rather than leaked.
        let tx = Arc::try_unwrap(scope)
            .map_err(|_| DatabaseError::query("an authorization body outlived its read scope"))?
            .into_transaction();

        match outcome {
            Ok(value) => {
                tx.commit()
                    .await
                    .map_err(|e| DatabaseError::query(e.to_string()))?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    warn!(error = %rollback, "rolling back a failed authorization evaluation failed");
                }
                Err(error)
            }
        }
    }

    async fn cluster_ts(&self) -> Result<HybridTs, AuthzError> {
        let ct = ClusterTime::read(&self.pool)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        Ok(HybridTs::new(ct.nanos(), u32::from(ct.logical())))
    }

    async fn query(
        &self,
        scope: &SqlReadScope,
        builder: QueryBuilder<RelationTupleRow, SqlDb>,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let mut tx = scope.tx().lock().await;
        Self::query_in(&mut tx, builder).await
    }

    /// Runs a filtered tuple query on `tx`: the one place stored rows become tuples.
    pub(super) async fn query_in(
        tx: &mut SqlTransaction,
        builder: QueryBuilder<RelationTupleRow, SqlDb>,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let rows = RepositoryTransactionQuery::query_where(tx, &builder).await?;
        rows.into_iter().map(RelationTupleRow::into_tuple).collect()
    }

    /// The point lookup of one exact edge, keyed as a write keys it.
    ///
    /// Built once for both scopes, so they cannot derive different tuple keys.
    pub(super) fn edge_query(
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> QueryBuilder<RelationTupleRow, SqlDb> {
        let key = RelationTuple::new(object.clone(), relation.clone(), subject.clone())
            .key()
            .as_str()
            .to_string();
        QueryBuilder::<RelationTupleRow, SqlDb>::new()
            .filter(Filter::eq(columns::TUPLE_KEY, key))
            .limit(1)
    }

    /// The edge an [`edge_query`](Self::edge_query) found, if it found one.
    pub(super) fn edge_of(tuples: Vec<RelationTuple>) -> Option<DirectEdge> {
        tuples
            .into_iter()
            .next()
            .map(|tuple| DirectEdge::from_caveat(tuple.caveat))
    }

    /// The userset edges stored on `object#relation`.
    pub(super) fn usersets_query(
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> QueryBuilder<RelationTupleRow, SqlDb> {
        Self::all_query(object, relation, limit).filter(Filter::ne(columns::SUBJECT_RELATION, ""))
    }

    /// Every edge stored on `object#relation`.
    pub(super) fn all_query(
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> QueryBuilder<RelationTupleRow, SqlDb> {
        QueryBuilder::<RelationTupleRow, SqlDb>::new()
            .filter(Filter::eq(columns::OBJECT_NS, object.namespace.as_str()))
            .filter(Filter::eq(columns::OBJECT_ID, object.id.as_str()))
            .filter(Filter::eq(columns::RELATION, relation.as_str()))
            .limit(limit as u64)
    }

    /// The tuples naming `subject` directly, optionally narrowed.
    pub(super) fn by_subject_query(
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> QueryBuilder<RelationTupleRow, SqlDb> {
        let subject_relation = subject
            .relation
            .as_ref()
            .map(RelationName::as_str)
            .unwrap_or("");
        let mut builder = QueryBuilder::<RelationTupleRow, SqlDb>::new()
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
        builder.limit(limit as u64)
    }
}

impl TupleStore for SqlTupleStore {
    fn backend(&self) -> BackendTag {
        BackendTag::SqlDb
    }

    type Scope = SqlReadScope;

    #[instrument(skip(self, body))]
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
        let mut attempt = 0;
        loop {
            match self.attempt(&body).await {
                Err(AuthzError::Storage(e)) if attempt < Self::READ_RESTARTS && is_restart(&e) => {
                    attempt += 1;
                    warn!(
                        attempt,
                        "authorization evaluation restarted at a fresh read point"
                    );
                }
                outcome => return outcome,
            }
        }
    }

    #[instrument(skip(self, scope), fields(object = %object, relation = %relation, subject = %subject))]
    async fn read_edge(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        let tuples = self
            .query(scope, Self::edge_query(object, relation, subject))
            .await?;
        Ok(Self::edge_of(tuples))
    }

    #[instrument(skip(self, scope), fields(object = %object, relation = %relation, limit))]
    async fn read_usersets(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.query(scope, Self::usersets_query(object, relation, limit))
            .await
    }

    #[instrument(skip(self, scope), fields(object = %object, relation = %relation, limit))]
    async fn read_all(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.query(scope, Self::all_query(object, relation, limit))
            .await
    }

    #[instrument(skip(self, scope), fields(subject = %subject, limit))]
    async fn read_by_subject(
        &self,
        scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.query(
            scope,
            Self::by_subject_query(subject, object_namespace, relation, limit),
        )
        .await
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let repo = self.pool.repository::<RelationTupleRow>();
        for tuple in tuples {
            repo.upsert(&RelationTupleRow::from_tuple(tuple)?).await?;
        }
        let ts = self.cluster_ts().await?;
        Ok(ts)
    }

    #[instrument(skip(self, tuples), fields(tuples = tuples.len()))]
    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        // One statement per key in key order, as a locked check locks; a multi-row `DELETE` locks
        // hash-sharded rows in no stated order.
        self.pool
            .transaction(Self::REVOCATION_ISOLATION, async |tx| {
                Self::delete_in(tx, tuples).await
            })
            .await?;
        let ts = self.cluster_ts().await?;
        Ok(ts)
    }
}

/// `tuples` in ascending [`TupleKey`] order, each key once and as given last.
///
/// The one order every tuple lock is taken in, so no two can deadlock.
fn in_key_order(tuples: &[RelationTuple]) -> impl Iterator<Item = &RelationTuple> {
    tuples
        .iter()
        .map(|tuple| (tuple.key(), tuple))
        .collect::<BTreeMap<TupleKey, &RelationTuple>>()
        .into_values()
}

/// Whether a storage failure is the server asking for the read to be taken again.
///
/// A restart request, not a decision; matched on the framework's classification.
fn is_restart(error: &DatabaseError) -> bool {
    error.is_retryable()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CaveatName;
    use crate::tuple::{CanonicalId, Caveat, CaveatContext};

    fn tuple(object: &str) -> RelationTuple {
        let ns = |n: &str| NamespaceName::new(n).expect("valid namespace");
        let id = |i: &str| CanonicalId::new(i).expect("valid id");
        RelationTuple::new(
            ObjectRef::new(ns("team"), id(object)),
            RelationName::new("member").expect("valid relation"),
            SubjectRef::leaf(ns("user"), id("alice")),
        )
    }

    #[test]
    fn writes_and_revocations_take_their_locks_in_key_order() {
        let given = [tuple("c"), tuple("a"), tuple("b")];
        let keys: Vec<TupleKey> = in_key_order(&given).map(RelationTuple::key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn a_tuple_given_twice_is_taken_once_as_given_last() {
        let caveated = tuple("a").with_caveat(Caveat::new(
            CaveatName::new("on_duty").expect("valid caveat"),
            CaveatContext::new(),
        ));
        let given = [tuple("a"), caveated.clone()];
        let taken: Vec<&RelationTuple> = in_key_order(&given).collect();
        assert_eq!(taken, vec![&caveated]);
    }
}
