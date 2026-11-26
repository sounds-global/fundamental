//! YCQL-specific repository extension traits.

use std::future::Future;

use fundamental_core::model::{BoundTo, CqlDb, CqlModel, Model};
use fundamental_db::{DatabaseResult, RepositoryTranslation};
use scylla::serialize::row::SerializeRow;

use crate::{BatchType, CqlResult, DeleteBatch, FromRow, InsertBatch, ToRow};

use super::CqlRepository;

/// Lightweight-transaction (LWT) writes: conditional `IF [NOT] EXISTS`.
///
/// `Ok(false)` is only the store's answer that the condition failed. A result that does not say
/// is `Err(`[`CqlError::LwtUnconfirmed`](crate::CqlError)`)`, which a single-use guard must treat
/// as unknown.
pub trait CqlLwt<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    /// Inserts a model if it doesn't exist (lightweight transaction).
    ///
    /// Returns `true` if the insert was applied (row did not exist),
    /// `false` if the row already existed.
    fn insert_if_not_exists(&self, model: &M) -> impl Future<Output = CqlResult<bool>> + Send;

    /// Inserts a model if it doesn't exist, with a per-write TTL — the atomic
    /// single-use-with-expiry primitive (`INSERT … IF NOT EXISTS USING TTL`).
    ///
    /// One lightweight transaction, so two concurrent callers with one key cannot both see `true`.
    /// Returns `true` if the insert was applied; writes no translation companion rows.
    fn insert_if_not_exists_with_ttl(
        &self,
        model: &M,
        ttl_seconds: i32,
    ) -> impl Future<Output = CqlResult<bool>> + Send;

    /// Updates a model if it exists (lightweight transaction).
    ///
    /// Returns `true` if the update was applied (row existed),
    /// `false` if the row did not exist.
    fn update_if_exists(&self, model: &M) -> impl Future<Output = CqlResult<bool>> + Send;

    /// Deletes a model with `IF EXISTS`, also removing its translation rows when the delete
    /// applies.
    ///
    /// Returns `true` if the delete was applied (row existed),
    /// `false` if the row did not exist.
    fn delete_if_exists(&self, model: &M) -> impl Future<Output = CqlResult<bool>> + Send
    where
        M::KeyValues: SerializeRow + Send + Sync;
}

impl<M> CqlLwt<M> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    async fn insert_if_not_exists(&self, model: &M) -> CqlResult<bool> {
        self.exec_insert(model, None, None, true).await
    }

    async fn insert_if_not_exists_with_ttl(&self, model: &M, ttl_seconds: i32) -> CqlResult<bool> {
        self.exec_insert(model, Some(ttl_seconds), None, true).await
    }

    async fn update_if_exists(&self, model: &M) -> CqlResult<bool> {
        self.exec_update(model, None, None, true).await
    }

    async fn delete_if_exists(&self, model: &M) -> CqlResult<bool>
    where
        M::KeyValues: SerializeRow + Send + Sync,
    {
        let key = model.to_key_values();
        let applied = self.exec_delete_keys(&key, true).await?;
        if applied {
            self.exec_delete_translations(&key).await?;
        }
        Ok(applied)
    }
}

/// Per-write TTL variants: `USING TTL <seconds>` on inserts and batches.
pub trait CqlTtl<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    /// Inserts a model with a TTL.
    fn insert_with_ttl(
        &self,
        model: &M,
        ttl_seconds: i32,
    ) -> impl Future<Output = CqlResult<()>> + Send;

    /// Updates a model with a TTL.
    fn update_with_ttl(
        &self,
        model: &M,
        ttl_seconds: i32,
    ) -> impl Future<Output = CqlResult<()>> + Send;

    /// Inserts multiple models with a TTL in a single batch operation.
    fn insert_batch_with_ttl(
        &self,
        models: &[M],
        batch_type: BatchType,
        ttl_seconds: i32,
    ) -> impl Future<Output = CqlResult<()>> + Send
    where
        M::InsertValues: SerializeRow + Send + Sync;
}

impl<M> CqlTtl<M> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    async fn insert_with_ttl(&self, model: &M, ttl_seconds: i32) -> CqlResult<()> {
        self.exec_insert(model, Some(ttl_seconds), None, false)
            .await?;
        Ok(())
    }

    async fn update_with_ttl(&self, model: &M, ttl_seconds: i32) -> CqlResult<()> {
        self.exec_update(model, Some(ttl_seconds), None, false)
            .await?;
        Ok(())
    }

    async fn insert_batch_with_ttl(
        &self,
        models: &[M],
        batch_type: BatchType,
        ttl_seconds: i32,
    ) -> CqlResult<()>
    where
        M::InsertValues: SerializeRow + Send + Sync,
    {
        self.exec_insert_batch(models, batch_type, Some(ttl_seconds))
            .await
    }
}

/// Event-time writes: `USING TIMESTAMP <micros>`, alone or with a TTL.
///
/// YCQL resolves two writes to one cell by write time, so stamping the event's time lets an older
/// or redelivered event lose. Pass microseconds since the epoch; never mix explicit and implicit
/// write times on one table. A lightweight transaction cannot take one: the server rejects it.
pub trait CqlWriteTime<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    /// Inserts a model stamped at `write_time_micros` instead of the coordinator's
    /// clock.
    fn insert_at(
        &self,
        model: &M,
        write_time_micros: i64,
    ) -> impl Future<Output = CqlResult<()>> + Send;

    /// Inserts a model with a TTL, stamped at `write_time_micros` — the
    /// expiring-projection write.
    ///
    /// The TTL still counts from now, not from the write time.
    fn insert_with_ttl_at(
        &self,
        model: &M,
        ttl_seconds: i32,
        write_time_micros: i64,
    ) -> impl Future<Output = CqlResult<()>> + Send;

    /// Updates a model stamped at `write_time_micros`.
    fn update_at(
        &self,
        model: &M,
        write_time_micros: i64,
    ) -> impl Future<Output = CqlResult<()>> + Send;
}

impl<M> CqlWriteTime<M> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    async fn insert_at(&self, model: &M, write_time_micros: i64) -> CqlResult<()> {
        self.exec_insert(model, None, Some(write_time_micros), false)
            .await?;
        Ok(())
    }

    async fn insert_with_ttl_at(
        &self,
        model: &M,
        ttl_seconds: i32,
        write_time_micros: i64,
    ) -> CqlResult<()> {
        self.exec_insert(model, Some(ttl_seconds), Some(write_time_micros), false)
            .await?;
        Ok(())
    }

    async fn update_at(&self, model: &M, write_time_micros: i64) -> CqlResult<()> {
        self.exec_update(model, None, Some(write_time_micros), false)
            .await?;
        Ok(())
    }
}

/// Multi-row batch writes and their builders.
pub trait CqlBatch<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    /// Inserts multiple models in a single batch operation.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{BatchType, CqlBatch, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     #[model(clustering_key)]
    /// #     bucket: i64,
    /// #     payload: String,
    /// # }
    /// # async fn example(pool: CqlPool, events: Vec<Event>) -> Result<(), Box<dyn std::error::Error>> {
    /// let repository = pool.repository::<Event>();
    /// repository.insert_batch(&events, BatchType::Logged).await?;
    /// # Ok(())
    /// # }
    /// ```
    fn insert_batch(
        &self,
        models: &[M],
        batch_type: BatchType,
    ) -> impl Future<Output = CqlResult<()>> + Send
    where
        M::InsertValues: SerializeRow + Send + Sync;

    /// Deletes multiple models by their keys in a single batch operation.
    fn delete_batch(
        &self,
        keys: Vec<M::KeyValues>,
        batch_type: BatchType,
    ) -> impl Future<Output = CqlResult<()>> + Send
    where
        M::KeyValues: SerializeRow + Send + Sync;

    /// Creates an insert batch builder for more control over batch configuration.
    fn insert_batch_builder(&self) -> InsertBatch<M>
    where
        M::InsertValues: SerializeRow + Send + Sync;

    /// Creates a delete batch builder for more control over batch configuration.
    fn delete_batch_builder(&self) -> DeleteBatch<M>
    where
        M::KeyValues: SerializeRow + Send + Sync;
}

impl<M> CqlBatch<M> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    async fn insert_batch(&self, models: &[M], batch_type: BatchType) -> CqlResult<()>
    where
        M::InsertValues: SerializeRow + Send + Sync,
    {
        self.exec_insert_batch(models, batch_type, None).await
    }

    async fn delete_batch(&self, keys: Vec<M::KeyValues>, batch_type: BatchType) -> CqlResult<()>
    where
        M::KeyValues: SerializeRow + Send + Sync,
    {
        self.exec_delete_batch(keys, batch_type).await
    }

    fn insert_batch_builder(&self) -> InsertBatch<M>
    where
        M::InsertValues: SerializeRow + Send + Sync,
    {
        self.build_insert_batch()
    }

    fn delete_batch_builder(&self) -> DeleteBatch<M>
    where
        M::KeyValues: SerializeRow + Send + Sync,
    {
        self.build_delete_batch()
    }
}

/// Search the translation companion table by an exact translated value.
///
/// Exact match only: YCQL has no `LIKE`, so this uses `ALLOW FILTERING`.
impl<M> RepositoryTranslation<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow
        + for<'frame, 'metadata> scylla::deserialize::row::DeserializeRow<'frame, 'metadata>
        + Clone
        + Send
        + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    type TranslationId = M::KeyValues;

    async fn find_ids_by_translation(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> DatabaseResult<Vec<M::KeyValues>> {
        Ok(self
            .exec_find_keys_by_translation(field, locale, value)
            .await?)
    }
}
