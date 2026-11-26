//! Batched insert operations.

use crate::CqlSession;
use std::{marker::PhantomData, sync::Arc};

use fundamental_core::i18n::TranslationRow;
use fundamental_core::model::{BoundTo, CqlDb, CqlModel};
use scylla::statement::batch::Batch;
use tracing::debug;

use crate::{BatchConfig, CqlError, CqlResult, InsertBuilder, PreparedCache, ToRow};

/// A batch of insert operations.
#[derive(Debug)]
pub struct InsertBatch<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow,
{
    session: CqlSession,
    cache: Arc<PreparedCache>,
    /// Empty means the session's `USE`d keyspace.
    keyspace: String,
    values: Vec<M::InsertValues>,
    /// Each localized record's key and the companion rows it holds, written once the rows
    /// are: empty for a model with no `Localized` text.
    translations: Vec<(M::KeyValues, Vec<TranslationRow>)>,
    config: BatchConfig,
    _marker: PhantomData<M>,
}

impl<M> InsertBatch<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + Send + Sync,
    M::InsertValues: scylla::serialize::row::SerializeRow + Send + Sync,
{
    /// Creates a new insert batch.
    pub fn new(session: CqlSession, cache: Arc<PreparedCache>, keyspace: String) -> Self {
        Self {
            session,
            cache,
            keyspace,
            values: Vec::new(),
            translations: Vec::new(),
            config: BatchConfig::default(),
            _marker: PhantomData,
        }
    }

    /// Creates a new insert batch with the given configuration.
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
            values: Vec::new(),
            translations: Vec::new(),
            config,
            _marker: PhantomData,
        }
    }

    /// Adds a model to the batch.
    pub fn add(&mut self, model: &M) {
        self.values.push(model.to_values());
        if M::translation_table().is_some() {
            self.translations
                .push((model.to_key_values(), model.translation_rows()));
        }
    }

    /// Adds multiple models to the batch.
    pub fn add_all<'a, I>(&mut self, models: I)
    where
        I: IntoIterator<Item = &'a M>,
        M: 'a,
    {
        for model in models {
            self.add(model);
        }
    }

    /// Returns the number of statements in the batch.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Executes the batch.
    ///
    /// If the batch exceeds the maximum size, it will be split into
    /// multiple smaller batches automatically.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(self) -> CqlResult<()> {
        if self.values.is_empty() {
            return Ok(());
        }

        let cql = if let Some(ttl) = self.config.ttl {
            InsertBuilder::new(&self.keyspace, M::TABLE)
                .columns_from::<M>()
                .ttl(ttl)
                .build()
        } else {
            InsertBuilder::new(&self.keyspace, M::TABLE)
                .columns_from::<M>()
                .build()
        };

        debug!(
            cql = %cql,
            count = self.values.len(),
            batch_type = ?self.config.batch_type,
            "Executing insert batch"
        );

        let stmt = self.cache.get_or_prepare(&cql).await?;

        for chunk in self.values.chunks(self.config.max_statements) {
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

        // Then each record's translations, in its own partition: cleared and written as a
        // single write is, not in the rows' batch, where a clear would share the writes'
        // time and win over them.
        for (key, rows) in self.translations {
            crate::translations::write_rows::<M>(
                &self.session,
                &self.cache,
                &self.keyspace,
                &key,
                rows,
            )
            .await?;
        }

        Ok(())
    }
}
