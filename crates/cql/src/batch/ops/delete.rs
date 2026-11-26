//! Batched delete operations.

use crate::CqlSession;
use std::{marker::PhantomData, sync::Arc};

use fundamental_core::model::{BoundTo, CqlDb, CqlModel};
use scylla::statement::batch::Batch;
use tracing::debug;

use crate::{BatchConfig, CqlError, CqlResult, DeleteBuilder, PreparedCache, ToRow};

/// A batch of delete operations.
#[derive(Debug)]
pub struct DeleteBatch<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow,
{
    session: CqlSession,
    cache: Arc<PreparedCache>,
    /// Empty means the session's `USE`d keyspace.
    keyspace: String,
    keys: Vec<M::KeyValues>,
    config: BatchConfig,
    _marker: PhantomData<M>,
}

impl<M> DeleteBatch<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + Send + Sync,
    M::KeyValues: scylla::serialize::row::SerializeRow + Send + Sync,
{
    /// Creates a new delete batch.
    pub fn new(session: CqlSession, cache: Arc<PreparedCache>, keyspace: String) -> Self {
        Self {
            session,
            cache,
            keyspace,
            keys: Vec::new(),
            config: BatchConfig::default(),
            _marker: PhantomData,
        }
    }

    /// Creates a new delete batch with the given configuration.
    pub fn with_config(
        session: CqlSession,
        cache: Arc<PreparedCache>,
        keyspace: String,
        config: BatchConfig,
    ) -> Self {
        Self {
            session,
            cache,
            keyspace,
            keys: Vec::new(),
            config,
            _marker: PhantomData,
        }
    }

    /// Adds a key to delete.
    pub fn add(&mut self, key: M::KeyValues) {
        self.keys.push(key);
    }

    /// Adds multiple keys to delete.
    pub fn add_all<I>(&mut self, keys: I)
    where
        I: IntoIterator<Item = M::KeyValues>,
    {
        self.keys.extend(keys);
    }

    /// Returns the number of statements in the batch.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Executes the batch.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(self) -> CqlResult<()> {
        if self.keys.is_empty() {
            return Ok(());
        }

        let cql = DeleteBuilder::new(&self.keyspace, M::TABLE)
            .where_columns_from::<M>()
            .build();

        debug!(
            cql = %cql,
            count = self.keys.len(),
            batch_type = ?self.config.batch_type,
            "Executing delete batch"
        );

        let stmt = self.cache.get_or_prepare(&cql).await?;

        for chunk in self.keys.chunks(self.config.max_statements) {
            let mut batch = Batch::new(self.config.batch_type.into());

            for _ in 0..chunk.len() {
                batch.append_statement((*stmt).clone());
            }

            self.session
                .inner()
                .batch(&batch, chunk)
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
        }

        // Then each record's translations, by the same key. Not in the rows' batch: the
        // engine refuses a batched statement that removes more than one row, and a record's
        // translations are a range of its partition.
        for key in &self.keys {
            crate::translations::delete_translations::<M>(
                &self.session,
                &self.cache,
                &self.keyspace,
                key,
            )
            .await?;
        }

        Ok(())
    }
}
