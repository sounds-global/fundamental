//! Async repository over the offline store, native only: each call runs the synchronous
//! [`OfflineStore`] inside [`tokio::task::spawn_blocking`].

use std::marker::PhantomData;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::task::spawn_blocking;
use tracing::{debug, error, instrument, warn};

use fundamental_core::model::{BoundTo, LeadingKeyAddressed, Offline, Satisfied};
use fundamental_db::{
    DatabaseError, DatabaseResult, KeyedRepository, KeyedRepositoryBatch, QueryBuilder, Repository,
    RepositoryAggregate, RepositoryBatch, RepositoryExt, RepositorySoftDelete,
    RepositoryTranslation,
};

use crate::db::{OfflineDatabase, OfflineTransaction, Value};
use crate::store::{OfflineRecord, OfflineStore, primary_key_value};

/// A shareable, thread-safe handle to an [`OfflineDatabase`].
#[derive(Debug, Clone)]
pub struct SharedOfflineDatabase(Arc<Mutex<OfflineDatabase>>);

impl SharedOfflineDatabase {
    /// Wraps an owned database in a shareable handle.
    pub fn new(db: OfflineDatabase) -> Self {
        Self(Arc::new(Mutex::new(db)))
    }

    /// Returns a repository for the model type `M`.
    pub fn repository<M>(&self) -> OfflineRepository<M> {
        OfflineRepository {
            db: self.0.clone(),
            _marker: PhantomData,
        }
    }

    /// Returns a query executor for the model type `M`.
    pub fn query_executor<M>(&self) -> crate::query::OfflineQueryExecutor<M> {
        crate::query::OfflineQueryExecutor::new(self.0.clone())
    }

    /// Returns a clone of the underlying `Arc<Mutex<…>>`.
    pub fn inner(&self) -> Arc<Mutex<OfflineDatabase>> {
        self.0.clone()
    }

    /// Runs `f` inside a single SQLite transaction on the shared connection.
    ///
    /// Commits on `Ok`, rolls back on `Err`. The lock is held for the whole closure, so
    /// `f` must not call the async repository methods: that deadlocks.
    /// # Errors
    ///
    /// Returns the closure's error (after rolling back), or a
    /// [`fundamental_db::DatabaseError`] if beginning or committing the
    /// transaction fails, or if the blocking task panics.
    #[instrument(level = "debug", skip(self, f))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub async fn transaction<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&OfflineTransaction<'_>) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<DatabaseError> + Send + 'static,
    {
        let db = self.0.clone();
        spawn_blocking(move || -> Result<T, E> {
            let guard = lock_db(&db);
            let tx = guard.transaction().map_err(DatabaseError::from)?;
            match f(&tx) {
                Ok(value) => {
                    tx.commit().map_err(DatabaseError::from)?;
                    debug!("offline transaction committed");
                    Ok(value)
                }
                Err(error) => {
                    debug!("offline transaction rolled back after closure error");
                    Err(error)
                }
            }
        })
        .await
        .map_err(join_error)?
    }
}

impl OfflineDatabase {
    /// Converts an owned database into a shareable [`SharedOfflineDatabase`].
    pub fn into_shared(self) -> SharedOfflineDatabase {
        SharedOfflineDatabase::new(self)
    }
}

/// Async repository for an offline-bound model `M`.
#[derive(Debug)]
pub struct OfflineRepository<M> {
    db: Arc<Mutex<OfflineDatabase>>,
    _marker: PhantomData<M>,
}

impl<M> Clone for OfflineRepository<M> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            _marker: PhantomData,
        }
    }
}

/// Maps a `spawn_blocking` join failure onto a [`DatabaseError`], logging its cause.
fn join_error(err: tokio::task::JoinError) -> DatabaseError {
    error!(error = %err, "offline blocking task failed (panicked or cancelled)");
    DatabaseError::query(format!("offline task panicked: {err}"))
}

/// Locks the shared database, recovering (and logging) a poisoned mutex: the connection is
/// still usable.
fn lock_db(db: &Mutex<OfflineDatabase>) -> MutexGuard<'_, OfflineDatabase> {
    db.lock().unwrap_or_else(|poisoned| {
        error!("offline db mutex was poisoned by a prior panic; recovering the guard");
        poisoned.into_inner()
    })
}

impl<M> Repository<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    type Id = Value;

    #[instrument(skip(self, model), fields(model = M::TYPE_NAME))]
    async fn insert(&self, model: &M) -> DatabaseResult<M> {
        let db = self.db.clone();
        let model = model.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).insert(&model)?;
            Ok::<M, crate::db::OfflineDbError>(model)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, model), fields(model = M::TYPE_NAME))]
    async fn update(&self, model: &M) -> DatabaseResult<M> {
        let db = self.db.clone();
        let model = model.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).upsert(&model)?;
            Ok::<M, crate::db::OfflineDbError>(model)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, model), fields(model = M::TYPE_NAME))]
    async fn upsert(&self, model: &M) -> DatabaseResult<M> {
        let db = self.db.clone();
        let model = model.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).upsert(&model)?;
            Ok::<M, crate::db::OfflineDbError>(model)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    fn query(&self) -> QueryBuilder<M, Offline> {
        QueryBuilder::new()
    }
}

impl<M> KeyedRepository<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(level = "debug", skip(self, id), fields(model = M::TYPE_NAME))]
    async fn find(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).get::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(level = "debug", skip(self, id), fields(model = M::TYPE_NAME))]
    async fn find_required(&self, id: &Self::Id) -> DatabaseResult<M> {
        self.find(id)
            .await?
            .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<id>"))
    }

    #[instrument(level = "debug", skip(self, id), fields(model = M::TYPE_NAME))]
    async fn exists(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).exists::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, id), fields(model = M::TYPE_NAME))]
    async fn delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).delete::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }
}

impl<M> RepositoryBatch<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(skip(self, models), fields(model = M::TYPE_NAME, count = models.len()))]
    async fn insert_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        let db = self.db.clone();
        let models = models.to_vec();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            let store = OfflineStore::new(&guard);
            for model in &models {
                store.insert(model)?;
            }
            Ok::<Vec<M>, crate::db::OfflineDbError>(models)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, models), fields(model = M::TYPE_NAME, count = models.len()))]
    async fn update_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        let db = self.db.clone();
        let models = models.to_vec();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            let store = OfflineStore::new(&guard);
            for model in &models {
                store.upsert(model)?;
            }
            Ok::<Vec<M>, crate::db::OfflineDbError>(models)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }
}

impl<M> KeyedRepositoryBatch<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(level = "debug", skip(self, ids), fields(model = M::TYPE_NAME, count = ids.len()))]
    async fn find_many(&self, ids: &[Self::Id]) -> DatabaseResult<Vec<M>> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            let store = OfflineStore::new(&guard);
            let mut out = Vec::new();
            for id in &ids {
                if let Some(model) = store.get::<M>(id)? {
                    out.push(model);
                }
            }
            Ok::<Vec<M>, crate::db::OfflineDbError>(out)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, ids), fields(model = M::TYPE_NAME, count = ids.len()))]
    async fn delete_many(&self, ids: &[Self::Id]) -> DatabaseResult<u64> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            let store = OfflineStore::new(&guard);
            let mut deleted = 0u64;
            for id in &ids {
                if store.delete::<M>(id)? {
                    deleted += 1;
                }
            }
            Ok::<u64, crate::db::OfflineDbError>(deleted)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }
}

impl<M> RepositoryAggregate<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(level = "debug", skip(self), fields(model = M::TYPE_NAME))]
    async fn count(&self) -> DatabaseResult<u64> {
        let db = self.db.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).count::<M>()
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(level = "debug", skip(self, _query), fields(model = M::TYPE_NAME))]
    async fn count_where<State>(
        &self,
        _query: &QueryBuilder<M, Offline, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        // There is no query-to-SQL translator here; refusing beats a wrong count.
        warn!("count_where is unsupported on the offline backend; rejecting (use count() instead)");
        Err(DatabaseError::query(
            "count_where is unsupported on the offline backend: no QueryBuilder→SQL \
             translator exists. Use `count()` for an unfiltered total, or query the \
             synchronous OfflineStore directly.",
        ))
    }
}

impl<M> RepositoryExt<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(level = "debug", skip(self, id, create), fields(model = M::TYPE_NAME))]
    async fn find_or_create<F>(&self, id: &Self::Id, create: F) -> DatabaseResult<M>
    where
        F: FnOnce() -> M + Send,
    {
        if let Some(found) = self.find(id).await? {
            return Ok(found);
        }
        self.insert(&create()).await
    }

    #[instrument(level = "debug", skip(self, model), fields(model = M::TYPE_NAME))]
    async fn update_if_exists(&self, model: &M) -> DatabaseResult<Option<M>> {
        let id = primary_key_value(model);
        if self.exists(&id).await? {
            Ok(Some(self.update(model).await?))
        } else {
            Ok(None)
        }
    }
}

impl<M> RepositoryTranslation<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    type TranslationId = Value;

    #[instrument(level = "debug", skip(self, value), fields(model = M::TYPE_NAME, field, locale))]
    async fn find_ids_by_translation(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> DatabaseResult<Vec<Self::TranslationId>> {
        let db = self.db.clone();
        let field = field.to_string();
        let locale = locale.to_string();
        let value = value.to_string();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).find_ids_by_translation::<M>(&field, &locale, &value)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }
}

impl<M> RepositorySoftDelete<M, Offline> for OfflineRepository<M>
where
    M: BoundTo<Offline> + OfflineRecord + Clone + Send + Sync + 'static,
{
    #[instrument(skip(self, id), fields(model = M::TYPE_NAME))]
    async fn soft_delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).soft_delete::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, id), fields(model = M::TYPE_NAME))]
    async fn restore(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).restore::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(level = "debug", skip(self, id), fields(model = M::TYPE_NAME))]
    async fn find_with_deleted(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).find_with_deleted::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }

    #[instrument(skip(self, id), fields(model = M::TYPE_NAME))]
    async fn purge(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let db = self.db.clone();
        let id = id.clone();
        spawn_blocking(move || {
            let guard = lock_db(&db);
            OfflineStore::new(&guard).purge::<M>(&id)
        })
        .await
        .map_err(join_error)?
        .map_err(Into::into)
    }
}
