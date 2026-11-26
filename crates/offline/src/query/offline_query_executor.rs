//! [`OfflineQueryExecutor`] — runs an abstract [`QueryBuilder`] against SQLite.

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use fundamental_core::model::{BoundTo, LeadingKeyAddressed, Offline, Satisfied};
use fundamental_db::{DatabaseError, DatabaseResult, FilterValue, QueryBuilder, QueryExecutor};
use tokio::task::spawn_blocking;

use super::filter_sql::{render_filters, render_order_limit, render_set};
use crate::db::{OfflineDatabase, Value};
use crate::store::{OfflineRecord, OfflineStore};

/// Executes the framework's abstract [`QueryBuilder`] against offline SQLite, through
/// the synchronous [`OfflineStore`] on a blocking task. Native targets only.
pub struct OfflineQueryExecutor<M> {
    db: Arc<Mutex<OfflineDatabase>>,
    _model: PhantomData<fn() -> M>,
}

impl<M> OfflineQueryExecutor<M> {
    pub(crate) fn new(db: Arc<Mutex<OfflineDatabase>>) -> Self {
        Self {
            db,
            _model: PhantomData,
        }
    }

    async fn with_db<T, F>(&self, work: F) -> DatabaseResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&OfflineDatabase) -> DatabaseResult<T> + Send + 'static,
    {
        let handle = self.db.clone();
        spawn_blocking(move || {
            let db = handle
                .lock()
                .map_err(|_| DatabaseError::query("offline database lock poisoned"))?;
            work(&db)
        })
        .await
        .map_err(|e| DatabaseError::query(format!("offline query task failed: {e}")))?
    }
}

impl<M> QueryExecutor<M, Offline> for OfflineQueryExecutor<M>
where
    M: OfflineRecord + BoundTo<Offline> + Send + Sync + 'static,
{
    async fn execute<State>(
        &self,
        query: &QueryBuilder<M, Offline, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        let mut next = 1;
        let mut params: Vec<Value> = Vec::new();
        let where_body = render_filters(query.get_filters(), &mut next, &mut params)?;
        let pagination = query.get_pagination();
        let order_limit = render_order_limit(
            query.get_order_by(),
            pagination.map(|p| p.limit),
            pagination.map(|p| p.offset),
        )?;
        self.with_db(move |db| {
            OfflineStore::new(db)
                .query_filtered::<M>(&where_body, &order_limit, &params)
                .map_err(DatabaseError::from)
        })
        .await
    }

    async fn execute_one<State>(
        &self,
        query: &QueryBuilder<M, Offline, State>,
    ) -> DatabaseResult<Option<M>>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        Ok(self.execute(query).await?.into_iter().next())
    }

    async fn count<State>(&self, query: &QueryBuilder<M, Offline, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        let mut next = 1;
        let mut params: Vec<Value> = Vec::new();
        let where_body = render_filters(query.get_filters(), &mut next, &mut params)?;
        self.with_db(move |db| {
            OfflineStore::new(db)
                .count_filtered::<M>(&where_body, &params)
                .map_err(DatabaseError::from)
        })
        .await
    }

    async fn delete_where<State>(
        &self,
        query: &QueryBuilder<M, Offline, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        let mut next = 1;
        let mut params: Vec<Value> = Vec::new();
        let where_body = render_filters(query.get_filters(), &mut next, &mut params)?;
        self.with_db(move |db| {
            OfflineStore::new(db)
                .delete_filtered::<M>(&where_body, &params)
                .map_err(DatabaseError::from)
        })
        .await
    }

    async fn update_where<State>(
        &self,
        query: &QueryBuilder<M, Offline, State>,
        updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, Offline, State>: Satisfied,
    {
        // SET placeholders are bound first, then the filter's.
        let mut next = 1;
        let mut params: Vec<Value> = Vec::new();
        let set_body = render_set(updates, &mut next, &mut params)?;
        let where_body = render_filters(query.get_filters(), &mut next, &mut params)?;
        self.with_db(move |db| {
            OfflineStore::new(db)
                .update_filtered::<M>(&set_body, &where_body, &params)
                .map_err(DatabaseError::from)
        })
        .await
    }
}
