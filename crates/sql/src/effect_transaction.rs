use crate::query::BindKey;
use crate::{FromRow, RowLockOutcome, SqlError, SqlQuery, SqlTransaction, ToRow};
use fundamental_core::Model;
use fundamental_core::model::{
    BoundTo, CompositeKeyModel, LeadingKeyAddressed, Satisfied, SqlDb, SqlModel,
};
use fundamental_db::{
    DatabaseResult, EffectModel, FilterValue, KeyedRepositoryTransaction,
    KeyedRepositoryTransactionBatch, QueryBuilder, RepositoryTransaction,
    RepositoryTransactionAggregate, RepositoryTransactionBatch, RepositoryTransactionQuery,
    RepositoryTransactionSoftDelete, TransactionScope,
};
use sqlx::Postgres;

/// A [`SqlTransaction`] narrowed to what an effect may do inside a transaction somebody else
/// owns: typed reads and writes of [`EffectModel`]s, row locks, savepoints and outbox rows, but
/// no commit, rollback, raw SQL or connection.
pub struct EffectTransaction<'t> {
    tx: &'t mut SqlTransaction,
}

impl<'t> EffectTransaction<'t> {
    /// Lends `tx` to an effect.
    #[must_use]
    pub fn scope(tx: &'t mut SqlTransaction) -> Self {
        Self { tx }
    }

    /// Inserts `model` unless a row already holds its `conflict_target` columns.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::insert_or_ignore`].
    pub async fn insert_or_ignore<M>(
        &mut self,
        model: &M,
        conflict_target: &[&str],
    ) -> Result<Option<M>, SqlError>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    {
        self.tx.insert_or_ignore(model, conflict_target).await
    }

    /// Reads `id` and locks its row until the transaction ends.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::find_for_update`].
    pub async fn find_for_update<M>(&mut self, id: &M::Id) -> DatabaseResult<Option<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        self.tx.find_for_update(id).await
    }

    /// Locks `id`'s row unless another transaction holds it, and never waits to find out.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::find_for_update_unless_held`].
    pub async fn find_for_update_unless_held<M>(
        &mut self,
        id: &M::Id,
    ) -> DatabaseResult<RowLockOutcome<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        self.tx.find_for_update_unless_held(id).await
    }

    /// Reads `ids` and locks their rows, in key order, until the transaction ends.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::find_many_for_update`].
    pub async fn find_many_for_update<M>(&mut self, ids: &[M::Id]) -> DatabaseResult<Vec<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M::Id:
            Ord + Clone + Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        self.tx.find_many_for_update(ids).await
    }

    /// Reads a composite-key row and locks it until the transaction ends.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::find_for_update_by_key`].
    pub async fn find_for_update_by_key<'k, M>(
        &mut self,
        key: &'k M::KeyValues,
    ) -> DatabaseResult<Option<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        self.tx.find_for_update_by_key(key).await
    }

    /// Reads a composite-key row.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::find_by_key`].
    pub async fn find_by_key<'k, M>(&mut self, key: &'k M::KeyValues) -> DatabaseResult<Option<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'k>,
    {
        self.tx.find_by_key(key).await
    }

    /// Reads the rows `query` matches and locks them until the transaction ends.
    ///
    /// # Errors
    ///
    /// As [`SqlTransaction::query_locked`].
    pub async fn query_locked<M, State>(
        &mut self,
        query: &SqlQuery<M, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        M: EffectModel + BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.tx.query_locked(query).await
    }
}

impl TransactionScope<SqlDb> for EffectTransaction<'_> {
    async fn savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.tx.savepoint(name).await
    }

    async fn rollback_to_savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.tx.rollback_to_savepoint(name).await
    }

    async fn release_savepoint(&mut self, name: &str) -> DatabaseResult<()> {
        self.tx.release_savepoint(name).await
    }
}

impl<M> RepositoryTransaction<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: RepositoryTransaction<M, SqlDb>,
{
    type Id = <SqlTransaction as RepositoryTransaction<M, SqlDb>>::Id;

    async fn insert(&mut self, model: &M) -> DatabaseResult<M> {
        self.tx.insert(model).await
    }

    async fn update(&mut self, model: &M) -> DatabaseResult<M> {
        self.tx.update(model).await
    }

    async fn upsert(&mut self, model: &M) -> DatabaseResult<M> {
        self.tx.upsert(model).await
    }

    async fn insert_if_absent(&mut self, model: &M) -> DatabaseResult<Option<M>> {
        self.tx.insert_if_absent(model).await
    }
}

impl<M> KeyedRepositoryTransaction<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: KeyedRepositoryTransaction<M, SqlDb>,
{
    async fn find(&mut self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        KeyedRepositoryTransaction::<M, SqlDb>::find(&mut *self.tx, id).await
    }

    async fn find_required(&mut self, id: &Self::Id) -> DatabaseResult<M> {
        KeyedRepositoryTransaction::<M, SqlDb>::find_required(&mut *self.tx, id).await
    }

    async fn exists(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        KeyedRepositoryTransaction::<M, SqlDb>::exists(&mut *self.tx, id).await
    }

    async fn delete(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        KeyedRepositoryTransaction::<M, SqlDb>::delete(&mut *self.tx, id).await
    }
}

impl<M> RepositoryTransactionBatch<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: RepositoryTransactionBatch<M, SqlDb>,
{
    async fn insert_many(&mut self, models: &[M]) -> DatabaseResult<Vec<M>> {
        self.tx.insert_many(models).await
    }

    async fn update_many(&mut self, models: &[M]) -> DatabaseResult<Vec<M>> {
        self.tx.update_many(models).await
    }
}

impl<M> KeyedRepositoryTransactionBatch<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: KeyedRepositoryTransactionBatch<M, SqlDb>,
{
    async fn find_many(&mut self, ids: &[Self::Id]) -> DatabaseResult<Vec<M>> {
        KeyedRepositoryTransactionBatch::<M, SqlDb>::find_many(&mut *self.tx, ids).await
    }

    async fn delete_many(&mut self, ids: &[Self::Id]) -> DatabaseResult<u64> {
        KeyedRepositoryTransactionBatch::<M, SqlDb>::delete_many(&mut *self.tx, ids).await
    }
}

impl<M> RepositoryTransactionQuery<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: RepositoryTransactionQuery<M, SqlDb>,
{
    async fn query_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.tx.query_where(query).await
    }

    async fn delete_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.tx.delete_where(query).await
    }

    async fn update_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
        updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.tx.update_where(query, updates).await
    }
}

impl<M> RepositoryTransactionAggregate<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: RepositoryTransactionAggregate<M, SqlDb>,
{
    async fn count(&mut self) -> DatabaseResult<u64> {
        RepositoryTransactionAggregate::<M, SqlDb>::count(&mut *self.tx).await
    }

    async fn count_where<State>(
        &mut self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.tx.count_where(query).await
    }
}

impl<M> RepositoryTransactionSoftDelete<M, SqlDb> for EffectTransaction<'_>
where
    M: EffectModel + BoundTo<SqlDb> + Send + Sync,
    SqlTransaction: RepositoryTransactionSoftDelete<M, SqlDb>,
{
    async fn soft_delete(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        RepositoryTransactionSoftDelete::<M, SqlDb>::soft_delete(&mut *self.tx, id).await
    }

    async fn restore(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        RepositoryTransactionSoftDelete::<M, SqlDb>::restore(&mut *self.tx, id).await
    }

    async fn find_with_deleted(&mut self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        RepositoryTransactionSoftDelete::<M, SqlDb>::find_with_deleted(&mut *self.tx, id).await
    }

    async fn purge(&mut self, id: &Self::Id) -> DatabaseResult<bool> {
        RepositoryTransactionSoftDelete::<M, SqlDb>::purge(&mut *self.tx, id).await
    }
}
