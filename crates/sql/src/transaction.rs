//! YSQL transaction wrapper.

use super::SqlError;
use crate::ident::is_valid_identifier;
use crate::model::key_predicate::key_predicate;
use crate::outbox::{ClaimedKey, ClaimedRow, SetAsideRow};
use crate::query::{BindKey, RowLock, pg_query};
use crate::repository_ops as ops;
use crate::{FromRow, RowLockOutcome, SqlQuery, ToRow};
use chrono::{DateTime, Utc};
use fundamental_core::Model;
use fundamental_core::model::{
    BoundTo, CompositeKeyModel, LeadingKeyAddressed, Satisfied, SqlDb, SqlModel,
};
use fundamental_core::outbox::{HeldCount, RefusalKind};
use fundamental_db::SchemaName;
use fundamental_db::{
    DatabaseError, DatabaseMetrics, DatabaseResult, FilterValue, IsolationLevel,
    KeyedRepositoryTransaction, KeyedRepositoryTransactionBatch, QueryBuilder,
    RepositoryTransaction, RepositoryTransactionAggregate, RepositoryTransactionBatch,
    RepositoryTransactionCount, RepositoryTransactionExt, RepositoryTransactionQuery,
    RepositoryTransactionSoftDelete, RepositoryTransactionTranslation, Transaction,
    TransactionScope,
};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use sqlx::{Postgres, Transaction as SqlxTransaction};
use std::sync::Arc;
use std::time::Instant;
use tracing::field::Empty;
use tracing::{Span, debug, error, instrument, warn};

/// A YSQL transaction, rolled back if dropped uncommitted; also a typed repository.
pub struct SqlTransaction {
    inner: Option<SqlxTransaction<'static, Postgres>>,
    key_manager: Option<SharedKeyManager>,
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    schema: SchemaName,
    started_at: Instant,
    /// Recorded where the conflict is born, since the caller's error type may have flattened it.
    conflict: Option<&'static str>,
    /// Set until a rollback to a savepoint: the server answers a later `COMMIT` with a silent rollback.
    aborted_by: Option<String>,
    /// `None` when the opening path named none, which a decision treats as the weakest.
    isolation: Option<IsolationLevel>,
    /// Only those opened through the savepoint methods; raw `SAVEPOINT` SQL is not counted.
    savepoints: Vec<String>,
}

/// The connection, schema, encryptor and SQLSTATE slot, borrowed disjointly from one transaction.
type StatementParts<'a> = (
    &'a mut sqlx::PgConnection,
    &'a SchemaName,
    Option<&'a KeyManager>,
    Notes<'a>,
);

struct Notes<'a> {
    conflict: &'a mut Option<&'static str>,
    aborted_by: &'a mut Option<String>,
}

/// Notes the SQLSTATE of a failed statement and hands the result back.
fn note<T>(notes: &mut Notes<'_>, out: Result<T, SqlError>) -> Result<T, SqlError> {
    if let Err(error) = &out {
        if let Some(code) = error.sqlstate() {
            *notes.conflict = Some(code);
        }
        if notes.aborted_by.is_none()
            && let Some(sqlstate) = error.aborting_sqlstate()
        {
            *notes.aborted_by = Some(sqlstate);
        }
    }
    out
}

impl SqlTransaction {
    pub(crate) fn new(tx: SqlxTransaction<'static, Postgres>) -> Self {
        Self {
            inner: Some(tx),
            key_manager: None,
            metrics: None,
            schema: SchemaName::default(),
            started_at: Instant::now(),
            conflict: None,
            aborted_by: None,
            isolation: None,
            savepoints: Vec::new(),
        }
    }

    /// Attaches the collector this transaction's outcome is recorded to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>) -> Self {
        metrics.record_transaction_start();
        self.metrics = Some(metrics);
        self
    }

    /// The SQLSTATE of the last retryable engine error this transaction produced.
    #[must_use]
    pub fn conflict(&self) -> Option<&'static str> {
        self.conflict
    }

    /// The SQLSTATE of the statement that aborted this transaction, or `None` while it can still commit.
    #[must_use]
    pub fn aborted_by(&self) -> Option<&str> {
        self.aborted_by.as_deref()
    }

    /// Records a statement run from elsewhere in the crate as this transaction's own.
    pub(crate) fn record<T>(&mut self, out: Result<T, SqlError>) -> Result<T, SqlError> {
        note(&mut self.notes(), out)
    }

    fn notes(&mut self) -> Notes<'_> {
        Notes {
            conflict: &mut self.conflict,
            aborted_by: &mut self.aborted_by,
        }
    }

    /// The isolation level this transaction was begun at, or `None` when the path that opened it named none.
    #[must_use]
    pub fn isolation(&self) -> Option<IsolationLevel> {
        self.isolation
    }

    /// How many savepoints are open on this transaction.
    #[must_use]
    pub fn open_savepoints(&self) -> usize {
        self.savepoints.len()
    }

    #[must_use]
    pub(crate) fn with_isolation(mut self, level: IsolationLevel) -> Self {
        self.isolation = Some(level);
        self
    }

    fn record_outcome(&self, committed: bool) {
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.record_transaction(committed, self.started_at.elapsed());
        }
    }

    /// Attaches a key manager, encrypting the model's `Secret` fields for typed CRUD in this transaction.
    #[must_use]
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Returns the inner transaction, or `None` once committed or rolled back.
    pub fn inner(&self) -> Option<&SqlxTransaction<'static, Postgres>> {
        self.inner.as_ref()
    }

    /// Returns a mutable reference to the inner transaction.
    pub fn inner_mut(&mut self) -> Option<&mut SqlxTransaction<'static, Postgres>> {
        self.inner.as_mut()
    }

    fn take(&mut self) -> Option<SqlxTransaction<'static, Postgres>> {
        self.inner.take()
    }

    /// The schema this transaction's statements qualify their tables with.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Qualifies this transaction's statements with `schema`.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaName) -> Self {
        self.schema = schema;
        self
    }

    /// The live connection and key manager for the shared query core.
    fn conn_and_key(&mut self) -> Result<StatementParts<'_>, SqlError> {
        let conn = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;
        Ok((
            &mut **conn,
            &self.schema,
            self.key_manager.as_deref(),
            Notes {
                conflict: &mut self.conflict,
                aborted_by: &mut self.aborted_by,
            },
        ))
    }

    /// Inserts a model, ignoring the write when it conflicts on `conflict_target`; `None` when a row already held those columns.
    ///
    /// # Errors
    /// [`SqlError::Query`] if `conflict_target` is empty or holds an invalid identifier, or the
    /// underlying database error on a write failure.
    #[instrument(skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, conflict_target = conflict_target.join(","), inserted = Empty), err(Debug))]
    pub async fn insert_or_ignore<M>(
        &mut self,
        model: &M,
        conflict_target: &[&str],
    ) -> Result<Option<M>, SqlError>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let outcome = note(
            &mut slot,
            ops::insert_or_ignore_with(conn, schema, enc, model, conflict_target).await,
        )?;
        Span::current().record("inserted", outcome.is_some());
        Ok(outcome)
    }

    /// Inserts `model`, or locks the row already stored under its key, in one statement, and returns the row as stored.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the statement fails.
    #[instrument(level = "debug", skip(self, model), fields(op = "tx_insert_or_lock", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn insert_or_lock<M>(&mut self, model: &M) -> DatabaseResult<M>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::insert_or_lock_with(conn, schema, enc, model).await,
        )?)
    }

    /// Finds a model by its primary key and takes a row lock on it for the rest of this transaction.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the query fails.
    #[instrument(level = "debug", skip(self, id), fields(op = "tx_find_for_update", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn find_for_update<M>(&mut self, id: &M::Id) -> DatabaseResult<Option<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        let found = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::find_for_update_with::<_, M>(conn, schema, enc, id, RowLock::ForUpdate).await,
            )?
        };
        Ok(found)
    }

    /// Takes the row lock [`find_for_update`](Self::find_for_update) takes, unless another
    /// transaction holds it, which is reported rather than waited on.
    ///
    /// # Errors
    ///
    /// [`DatabaseError::Configuration`] (`DB_009`) unless this transaction was begun at
    /// [`IsolationLevel::ReadCommitted`] or [`IsolationLevel::RepeatableRead`], since YugabyteDB
    /// ignores `SKIP LOCKED` under `SERIALIZABLE`. Otherwise as
    /// [`find_for_update`](Self::find_for_update).
    #[instrument(level = "debug", skip(self, id), fields(op = "tx_find_for_update_unless_held", table = M::TABLE, model = M::TYPE_NAME, outcome = Empty), err(Debug))]
    pub async fn find_for_update_unless_held<M>(
        &mut self,
        id: &M::Id,
    ) -> DatabaseResult<RowLockOutcome<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        match self.isolation {
            Some(IsolationLevel::ReadCommitted | IsolationLevel::RepeatableRead) => {}
            other => {
                return Err(DatabaseError::configuration(format!(
                    "find_for_update_unless_held on {} needs a READ COMMITTED or REPEATABLE \
                     READ transaction, and this one was begun at {other:?}: YugabyteDB ignores \
                     SKIP LOCKED under SERIALIZABLE and waits for the lock",
                    M::TABLE
                )));
            }
        }
        let outcome = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            let locked = note(
                &mut slot,
                ops::find_for_update_with::<_, M>(
                    &mut *conn,
                    schema,
                    enc,
                    id,
                    RowLock::ForUpdateSkipLocked,
                )
                .await,
            )?;
            match locked {
                Some(row) => RowLockOutcome::Locked(row),
                None => match note(
                    &mut slot,
                    ops::find_with::<_, M>(&mut *conn, schema, enc, id).await,
                )? {
                    Some(row) => RowLockOutcome::HeldElsewhere(row),
                    None => RowLockOutcome::Absent,
                },
            }
        };
        Span::current().record(
            "outcome",
            match &outcome {
                RowLockOutcome::Locked(_) => "locked",
                RowLockOutcome::HeldElsewhere(_) => "held_elsewhere",
                RowLockOutcome::Absent => "absent",
            },
        );
        Ok(outcome)
    }

    /// Finds every model whose primary key is in `ids` and locks each, in one statement, in key order so overlapping callers cannot deadlock.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed, `ids` holds more than
    /// 1 000 distinct keys, or the query fails.
    #[instrument(level = "debug", skip(self, ids), fields(op = "tx_find_many_for_update", table = M::TABLE, model = M::TYPE_NAME, ids = ids.len()), err(Debug))]
    pub async fn find_many_for_update<M>(&mut self, ids: &[M::Id]) -> DatabaseResult<Vec<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id:
            Ord + Clone + Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        let rows = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::find_many_for_update_with::<_, M>(conn, schema, enc, ids).await,
            )?
        };
        Ok(rows)
    }

    /// Finds a row by its whole composite key and takes a row lock on it for the rest of this transaction.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or a query fails.
    #[instrument(level = "debug", skip(self, key), fields(op = "tx_find_for_update_by_key", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn find_for_update_by_key<'k, M>(
        &mut self,
        key: &'k M::KeyValues,
    ) -> DatabaseResult<Option<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        let sql = format!(
            "{} WHERE {}{} FOR UPDATE",
            crate::model::translations::select_from::<M>(&self.schema),
            key_predicate::<M>()?,
            ops::soft_delete_and::<M>(),
        );
        // The encryptor is used: a composite-keyed model may carry encrypted columns.
        let found = {
            let (conn, _schema, enc, mut slot) = self.conn_and_key()?;
            let row = note(
                &mut slot,
                key.bind_all(pg_query(sql))
                    .fetch_optional(conn)
                    .await
                    .map_err(SqlError::from),
            )?;
            row.map(|row| crate::model::translations::decode::<M>(&row, enc))
                .transpose()?
        };
        Ok(found)
    }

    /// Finds a row by its whole composite key inside the transaction, without locking.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or a query fails.
    #[instrument(level = "debug", skip(self, key), fields(op = "tx_find_by_key", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn find_by_key<'k, M>(&mut self, key: &'k M::KeyValues) -> DatabaseResult<Option<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        let sql = format!(
            "{} WHERE {}{} LIMIT 1",
            crate::model::translations::select_from::<M>(&self.schema),
            key_predicate::<M>()?,
            ops::soft_delete_and::<M>(),
        );
        let found = {
            let (conn, _schema, enc, mut slot) = self.conn_and_key()?;
            let row = note(
                &mut slot,
                key.bind_all(pg_query(sql))
                    .fetch_optional(conn)
                    .await
                    .map_err(SqlError::from),
            )?;
            row.map(|row| crate::model::translations::decode::<M>(&row, enc))
                .transpose()?
        };
        Ok(found)
    }

    /// Soft-deletes a row by its whole composite key while it is live, returning whether this call deleted it.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed, the model is not declared
    /// `#[model(soft_delete)]`, or the statement fails.
    #[instrument(skip(self, key), fields(op = "tx_soft_delete_by_key", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn soft_delete_by_key<'k, M>(&mut self, key: &'k M::KeyValues) -> DatabaseResult<bool>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::soft_delete_by_key_with::<_, M>(conn, schema, key).await,
        )?)
    }

    /// Restores a soft-deleted row by its whole composite key, returning whether this call restored it.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed, the model is not declared
    /// `#[model(soft_delete)]`, or the statement fails.
    #[instrument(skip(self, key), fields(op = "tx_restore_by_key", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    pub async fn restore_by_key<'k, M>(&mut self, key: &'k M::KeyValues) -> DatabaseResult<bool>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::restore_by_key_with::<_, M>(conn, schema, key).await,
        )?)
    }

    /// Runs a [`SqlQuery`] within the transaction, honouring any row-locking clause it carries.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the query fails.
    #[instrument(level = "debug", skip(self, query), fields(op = "tx_query_locked", table = M::TABLE, model = M::TYPE_NAME, rows = Empty), err(Debug))]
    pub async fn query_locked<M, State>(
        &mut self,
        query: &SqlQuery<M, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let rows = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::select_locked_with(conn, schema, enc, query).await,
            )?
        };
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Runs an outbox claim's locked read, decoding each row on its own after copying its key.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed, the read fails, a key cannot
    /// be addressed, or a row fails to decode for a reason that is not the row's own refusal.
    #[instrument(level = "debug", skip(self, query), fields(op = "tx_query_claimable", table = M::TABLE, rows = Empty), err(Debug))]
    pub(crate) async fn query_claimable<M, State>(
        &mut self,
        query: &SqlQuery<M, State>,
    ) -> DatabaseResult<Vec<ClaimedRow<M>>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let rows = note(
            &mut slot,
            crate::outbox::select_claimable_with(conn, schema, enc, query).await,
        )?;
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Sets a claimed row aside in the framework's own columns, addressed by `key`.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the write does not land on
    /// exactly one row.
    pub(crate) async fn set_aside_claimed<M: SqlModel>(
        &mut self,
        key: &ClaimedKey,
        kind: RefusalKind,
        at: DateTime<Utc>,
    ) -> DatabaseResult<()> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        note(
            &mut slot,
            crate::outbox::mark_set_aside_with::<_, M>(conn, schema, enc, key, kind, at).await,
        )?;
        Ok(())
    }

    /// Leaves a claimed row for another process until `until`, addressed by `key`.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the write does not land on
    /// exactly one row.
    pub(crate) async fn skip_claimed<M: SqlModel>(
        &mut self,
        key: &ClaimedKey,
        until: DateTime<Utc>,
    ) -> DatabaseResult<()> {
        let (conn, schema, _, mut slot) = self.conn_and_key()?;
        note(
            &mut slot,
            crate::outbox::mark_skipped_with::<_, M>(conn, schema, key, until).await,
        )?;
        Ok(())
    }

    /// Writes `model` whole, by its key, only while the stored row still matches `fence`; `None` when it no longer did.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the write fails.
    #[instrument(level = "debug", skip(self, model, fence), fields(op = "tx_update_fenced", table = M::TABLE, written = Empty), err(Debug))]
    pub(crate) async fn update_fenced<M, State>(
        &mut self,
        model: &M,
        fence: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Option<M>>
    where
        M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let written = note(
            &mut slot,
            ops::update_fenced_with(conn, schema, enc, model, fence).await,
        )?;
        Span::current().record("written", written.is_some());
        Ok(written)
    }

    /// Reads up to `cap` of `query`'s set-aside rows without decoding any of them.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the read fails.
    pub(crate) async fn read_set_aside<M: SqlModel, State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
        cap: u64,
    ) -> DatabaseResult<Vec<SetAsideRow>> {
        let (conn, schema, _, mut slot) = self.conn_and_key()?;
        let rows = note(
            &mut slot,
            crate::outbox::read_set_aside_with(conn, schema, query, cap).await,
        )?;
        Ok(rows)
    }

    /// Counts `query`'s rows, reading no more than `cap`, and says whether it stopped there.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed or the count fails.
    pub async fn count_held<M: SqlModel, State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
        cap: u64,
    ) -> DatabaseResult<HeldCount> {
        let (conn, schema, _, mut slot) = self.conn_and_key()?;
        let held = note(
            &mut slot,
            crate::outbox::count_held_with(conn, schema, query, cap).await,
        )?;
        Ok(held)
    }

    /// Clears the set-aside columns of up to `batch` of `query`'s set-aside rows.
    ///
    /// # Errors
    /// Returns a [`DatabaseError`] if the transaction has completed, the rows cannot be locked or
    /// a clear does not land on exactly the row it locked.
    pub(crate) async fn requeue_set_aside<M: SqlModel, State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
        batch: u64,
    ) -> DatabaseResult<u64> {
        let (conn, schema, _, mut slot) = self.conn_and_key()?;
        let requeued = note(
            &mut slot,
            crate::outbox::requeue_set_aside_with(conn, schema, query, batch).await,
        )?;
        Ok(requeued)
    }

    /// Executes a raw SQL query within the transaction.
    #[instrument(level = "debug", skip(self, sql), fields(op = "tx_execute", rows_affected = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(&mut self, sql: &str) -> Result<u64, SqlError> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        let out = sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut **tx)
            .await;
        let result = note(&mut self.notes(), out.map_err(SqlError::from))?;
        let affected = result.rows_affected();
        Span::current().record("rows_affected", affected);
        Ok(affected)
    }

    /// Executes a raw SQL query and returns rows.
    #[instrument(level = "debug", skip(self, sql), fields(op = "tx_fetch_all", rows = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_all(&mut self, sql: &str) -> Result<Vec<sqlx::postgres::PgRow>, SqlError> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        let out = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut **tx)
            .await;
        let rows = note(&mut self.notes(), out.map_err(SqlError::from))?;
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Executes a raw SQL query and returns a single row.
    #[instrument(
        level = "debug",
        skip(self, sql),
        fields(op = "tx_fetch_one"),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_one(&mut self, sql: &str) -> Result<sqlx::postgres::PgRow, SqlError> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        let out = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&mut **tx)
            .await;
        let row = note(&mut self.notes(), out.map_err(SqlError::from))?;
        Ok(row)
    }

    /// Executes a raw SQL query and returns an optional row.
    #[instrument(level = "debug", skip(self, sql), fields(op = "tx_fetch_optional", found = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn fetch_optional(
        &mut self,
        sql: &str,
    ) -> Result<Option<sqlx::postgres::PgRow>, SqlError> {
        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        let out = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_optional(&mut **tx)
            .await;
        let row = note(&mut self.notes(), out.map_err(SqlError::from))?;
        Span::current().record("found", row.is_some());
        Ok(row)
    }

    /// Creates a savepoint with the given name, which must be a valid SQL identifier.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use fundamental_db::{IsolationLevel, Transaction, Transactional};
    /// # async fn risky_operation(
    /// #     _tx: &mut fundamental_sql::SqlTransaction,
    /// # ) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    /// # async fn example(pool: &fundamental_sql::SqlPool) -> Result<(), Box<dyn std::error::Error>> {
    /// let mut tx = pool.begin(IsolationLevel::Serializable).await?;
    /// tx.execute_savepoint("before_risky_operation").await?;
    /// if risky_operation(&mut tx).await.is_err() {
    ///     tx.execute_rollback_to_savepoint("before_risky_operation").await?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute_savepoint(&mut self, name: &str) -> Result<(), SqlError> {
        if !is_valid_identifier(name) {
            return Err(SqlError::Query(format!(
                "Invalid savepoint name: '{}'. Use alphanumeric characters and underscores only.",
                name
            )));
        }

        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        debug!(savepoint = name, "Creating savepoint");

        let sql = format!("SAVEPOINT \"{}\"", name);
        let out = pg_query(sql).execute(&mut **tx).await;
        note(&mut self.notes(), out.map_err(SqlError::from))?;
        self.savepoints.push(name.to_owned());

        Ok(())
    }

    /// Rolls back to a previously created savepoint, keeping the transaction active.
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub async fn execute_rollback_to_savepoint(&mut self, name: &str) -> Result<(), SqlError> {
        if !is_valid_identifier(name) {
            return Err(SqlError::Query(format!(
                "Invalid savepoint name: '{}'. Use alphanumeric characters and underscores only.",
                name
            )));
        }

        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        debug!(savepoint = name, "Rolling back to savepoint");

        let sql = format!("ROLLBACK TO SAVEPOINT \"{}\"", name);
        let out = pg_query(sql).execute(&mut **tx).await;
        note(&mut self.notes(), out.map_err(SqlError::from))?;
        if let Some(at) = self.savepoints.iter().rposition(|open| open == name) {
            self.savepoints.truncate(at + 1);
        }
        if let Some(sqlstate) = self.aborted_by.take() {
            debug!(
                savepoint = name,
                sqlstate, "the rollback recovered an aborted transaction"
            );
        }

        Ok(())
    }

    /// Releases a savepoint.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute_release_savepoint(&mut self, name: &str) -> Result<(), SqlError> {
        if !is_valid_identifier(name) {
            return Err(SqlError::Query(format!(
                "Invalid savepoint name: '{}'. Use alphanumeric characters and underscores only.",
                name
            )));
        }

        let tx = self
            .inner
            .as_mut()
            .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;

        debug!(savepoint = name, "Releasing savepoint");

        let sql = format!("RELEASE SAVEPOINT \"{}\"", name);
        let out = pg_query(sql).execute(&mut **tx).await;
        note(&mut self.notes(), out.map_err(SqlError::from))?;
        if let Some(at) = self.savepoints.iter().rposition(|open| open == name) {
            self.savepoints.truncate(at);
        }

        Ok(())
    }
}

impl Transaction<SqlDb> for SqlTransaction {
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    async fn commit(mut self) -> DatabaseResult<()> {
        if let Some(sqlstate) = self.aborted_by.take() {
            if let Some(tx) = self.take()
                && let Err(error) = tx.rollback().await
            {
                warn!(error = %error, sqlstate, "rollback of an aborted transaction failed");
            }
            self.record_outcome(false);
            error!(
                sqlstate,
                code = "SQL_019",
                "a statement aborted the transaction and its body returned Ok; not committed"
            );
            return Err(SqlError::TransactionAborted { sqlstate }.into());
        }
        if let Some(tx) = self.take() {
            debug!("Committing YSQL transaction");
            tx.commit().await.map_err(|e| {
                let err: SqlError = e.into();
                err
            })?;
            self.record_outcome(true);
        }
        Ok(())
    }

    async fn rollback(mut self) -> DatabaseResult<()> {
        if let Some(tx) = self.take() {
            debug!("Rolling back YSQL transaction");
            tx.rollback().await.map_err(|e| {
                let err: SqlError = e.into();
                err
            })?;
            self.record_outcome(false);
        }
        Ok(())
    }
}

impl TransactionScope<SqlDb> for SqlTransaction {
    async fn savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.execute_savepoint(name).await.map_err(Into::into)
    }

    async fn rollback_to_savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.execute_rollback_to_savepoint(name)
            .await
            .map_err(Into::into)
    }

    async fn release_savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.execute_release_savepoint(name)
            .await
            .map_err(Into::into)
    }
}

impl<M> RepositoryTransaction<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    // Not `Encode + Type`: that would make `insert`/`update`/`upsert` unavailable to composite-key models.
    M::Id: Send + Sync,
{
    type Id = M::Id;

    #[instrument(skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn insert(&mut self, model: &M) -> DatabaseResult<M> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::insert_with(conn, schema, enc, model).await,
        )?)
    }

    #[instrument(skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn update(&mut self, model: &M) -> DatabaseResult<M> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::update_with(conn, schema, enc, model).await,
        )?)
    }

    #[instrument(skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn upsert(&mut self, model: &M) -> DatabaseResult<M> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::upsert_with(conn, schema, enc, model).await,
        )?)
    }

    #[instrument(skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, inserted = Empty), err(Debug))]
    async fn insert_if_absent(&mut self, model: &M) -> DatabaseResult<Option<M>> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let outcome = note(
            &mut slot,
            ops::insert_or_ignore_with(conn, schema, enc, model, M::KEY_COLUMNS).await,
        )?;
        Span::current().record("inserted", outcome.is_some());
        Ok(outcome)
    }
}

impl<M> KeyedRepositoryTransaction<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    #[instrument(level = "debug", skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find(&mut self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        let found = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::find_with::<_, M>(conn, schema, enc, id).await,
            )?
        };
        Ok(found)
    }

    #[instrument(level = "debug", skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME))]
    async fn find_required(&mut self, id: &Self::Id) -> DatabaseResult<M> {
        self.find(id)
            .await?
            .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<id>".to_string()))
    }

    #[instrument(level = "debug", skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn exists(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::exists_with::<_, M>(conn, schema, id).await,
        )?)
    }

    #[instrument(skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn delete(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::delete_with::<_, M>(conn, schema, id).await,
        )?)
    }
}

impl<M> RepositoryTransactionBatch<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync,
{
    #[instrument(skip(self, models), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn insert_many(&mut self, models: &[M]) -> DatabaseResult<Vec<M>> {
        const CHUNK_SIZE: usize = 1000;
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let mut all_results = Vec::with_capacity(models.len());
        for chunk in models.chunks(CHUNK_SIZE) {
            all_results.extend(note(
                &mut slot,
                ops::insert_chunk_with(&mut *conn, schema, enc, chunk).await,
            )?);
        }
        Ok(all_results)
    }

    #[instrument(skip(self, models), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn update_many(&mut self, models: &[M]) -> DatabaseResult<Vec<M>> {
        const CHUNK_SIZE: usize = 500;
        crate::model::translations::whole_rows_only::<M>("update_many")?;
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        let mut all_results = Vec::with_capacity(models.len());
        for chunk in models.chunks(CHUNK_SIZE) {
            all_results.extend(note(
                &mut slot,
                ops::update_chunk_with(&mut *conn, schema, enc, chunk).await,
            )?);
        }
        Ok(all_results)
    }
}

impl<M> KeyedRepositoryTransactionBatch<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    #[instrument(level = "debug", skip(self, ids), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn find_many(&mut self, ids: &[Self::Id]) -> DatabaseResult<Vec<M>> {
        let rows = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::find_many_with::<_, M>(conn, schema, enc, ids).await,
            )?
        };
        Ok(rows)
    }

    #[instrument(skip(self, ids), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn delete_many(&mut self, ids: &[Self::Id]) -> DatabaseResult<u64> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::delete_many_with::<_, M>(conn, schema, ids).await,
        )?)
    }
}

impl<M> RepositoryTransactionAggregate<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    // `Send + Sync` only: neither method takes an id, and an `Encode` bound would exclude composite-key models.
    M::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, count = Empty), err(Debug))]
    async fn count(&mut self) -> DatabaseResult<u64> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        let count = note(&mut slot, ops::count_with::<_, M>(conn, schema).await)?;
        Span::current().record("count", count);
        Ok(count)
    }

    #[instrument(level = "debug", skip(self, query), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn count_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::count_where_with(conn, schema, query).await,
        )?)
    }
}

impl<M> RepositoryTransactionCount<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self, codes), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, at, count = Empty), err(Debug))]
    async fn count_between(
        &mut self,
        at: &'static str,
        from: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
        codes: Vec<fundamental_db::Filter>,
    ) -> DatabaseResult<u64> {
        let query = QueryBuilder::<M, SqlDb>::new()
            .filters(codes)
            .filter(fundamental_db::Filter::gte(at, from))
            .filter(fundamental_db::Filter::lt(at, until));
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        let count = note(&mut slot, ops::count_where_with(conn, schema, &query).await)?;
        Span::current().record("count", count);
        Ok(count)
    }
}

impl<M> RepositoryTransactionQuery<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    // `Send + Sync` only: `LeadingKeyAddressed` proves the query; `Encode` would exclude composite-key models.
    M::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self, query), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, rows = Empty), err(Debug))]
    async fn query_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let rows = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::select_where_with(conn, schema, enc, query).await,
            )?
        };
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    #[instrument(level = "debug", skip(self, query), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, affected = Empty), err(Debug))]
    async fn delete_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        let affected = note(
            &mut slot,
            ops::delete_where_with::<_, M, _>(conn, schema, query).await,
        )?;
        Span::current().record("affected", affected);
        Ok(affected)
    }

    #[instrument(level = "debug", skip(self, query, updates), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, columns = updates.len(), affected = Empty), err(Debug))]
    async fn update_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
        updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        let affected = note(
            &mut slot,
            ops::update_where_with::<_, M, _>(conn, schema, query, updates).await,
        )?;
        Span::current().record("affected", affected);
        Ok(affected)
    }
}

impl<M> RepositoryTransactionSoftDelete<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    #[instrument(skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn soft_delete(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::soft_delete_with::<_, M>(conn, schema, id).await,
        )?)
    }

    #[instrument(skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn restore(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::restore_with::<_, M>(conn, schema, id).await,
        )?)
    }

    #[instrument(level = "debug", skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find_with_deleted(&mut self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        let found = {
            let (conn, schema, enc, mut slot) = self.conn_and_key()?;
            note(
                &mut slot,
                ops::find_with_deleted_with::<_, M>(conn, schema, enc, id).await,
            )?
        };
        Ok(found)
    }

    #[instrument(skip(self, id), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn purge(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        let (conn, schema, _enc, mut slot) = self.conn_and_key()?;
        Ok(note(
            &mut slot,
            ops::delete_with::<_, M>(conn, schema, id).await,
        )?)
    }
}

impl<M> RepositoryTransactionExt<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    #[instrument(level = "debug", skip(self, model), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME))]
    async fn update_if_exists(&mut self, model: &M) -> DatabaseResult<Option<M>> {
        let (conn, schema, enc, mut slot) = self.conn_and_key()?;
        if note(
            &mut slot,
            ops::exists_with::<_, M>(&mut *conn, schema, model.id()).await,
        )? {
            Ok(Some(note(
                &mut slot,
                ops::update_with(&mut *conn, schema, enc, model).await,
            )?))
        } else {
            Ok(None)
        }
    }
}

impl<M> RepositoryTransactionTranslation<M, SqlDb> for SqlTransaction
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send
        + Sync
        + Unpin
        + for<'q> sqlx::Encode<'q, Postgres>
        + for<'r> sqlx::Decode<'r, Postgres>
        + sqlx::Type<Postgres>,
{
    type TranslationId = M::Id;

    #[instrument(level = "debug", skip(self, value), fields(op = "tx", table = M::TABLE, model = M::TYPE_NAME, field, locale), err(Debug))]
    async fn find_ids_by_translation(
        &mut self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> DatabaseResult<Vec<Self::TranslationId>> {
        if M::translation_table().is_none() {
            return Ok(Vec::new());
        }
        let conn = crate::SqlConn::from(self);
        crate::find_translation_ids::<M>(conn, field, locale, value).await
    }
}

impl Drop for SqlTransaction {
    fn drop(&mut self) {
        if self.inner.is_some() {
            debug!("YSQL transaction dropped without commit, will rollback");
        }
    }
}

impl std::fmt::Debug for SqlTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlTransaction")
            .field("open", &self.inner.is_some())
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .finish()
    }
}
