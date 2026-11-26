use std::{marker::PhantomData, ops::Bound, sync::Arc, time::Duration};

use crate::CqlSession;
use fundamental_core::model::{BoundTo, CqlDb, CqlModel, SinglePartitionModel};
use fundamental_db::{DatabaseError, DatabaseResult};
use fundamental_encryption::SharedKeyManager;
use tracing::warn;

use crate::{
    ClusteringFilter, ClusteringOp, Consistency, CqlResult, CqlValue, CqlValues, FromRow,
    HasPartitionKey, NoPartitionKey, PartitionValues, PreparedCache, ToRow, build_select_cql,
};

/// A YCQL query builder with typestate-enforced partition key.
///
/// The `State` type parameter tracks whether a partition key has been
/// specified. Only queries with `State = HasPartitionKey` can be executed.
#[derive(Debug)]
pub struct CqlQuery<M, State = NoPartitionKey>
where
    M: BoundTo<CqlDb> + CqlModel,
{
    session: CqlSession,
    cache: Arc<PreparedCache>,
    /// Empty means the session's `USE`d keyspace.
    keyspace: String,
    /// Decrypts `Secret` columns when rows are materialized.
    key_manager: Option<SharedKeyManager>,
    partition_key: Vec<CqlValue>,
    clustering_filters: Vec<ClusteringFilter>,
    /// Regular column filters.
    filters: Vec<(String, ClusteringOp)>,
    /// SELECT columns (None = all).
    select_columns: Option<Vec<String>>,
    limit: Option<u32>,
    consistency: Consistency,
    allow_filtering: bool,
    timeout: Option<Duration>,
    _model: PhantomData<M>,
    _state: PhantomData<State>,
}

impl<M> CqlQuery<M, NoPartitionKey>
where
    M: BoundTo<CqlDb> + CqlModel,
{
    /// Creates a new query builder.
    pub(crate) fn new(
        session: CqlSession,
        cache: Arc<PreparedCache>,
        keyspace: String,
        key_manager: Option<SharedKeyManager>,
    ) -> Self {
        Self {
            session,
            cache,
            keyspace,
            key_manager,
            partition_key: Vec::new(),
            clustering_filters: Vec::new(),
            filters: Vec::new(),
            select_columns: None,
            limit: None,
            consistency: Consistency::default(),
            allow_filtering: false,
            timeout: None,
            _model: PhantomData,
            _state: PhantomData,
        }
    }

    /// Addresses one partition, by the model's own [`PartitionKey`] values.
    ///
    /// `M::PartitionKey` is the tuple of the partition columns' types, so a call with the wrong
    /// number of values does not compile; two columns sharing a type can still be swapped
    /// ([`PartitionValues`](crate::PartitionValues)).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::CqlPool;
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "vault_rows")]
    /// # struct VaultRow {
    /// #     #[model(partition_key)]
    /// #     zone: String,
    /// #     #[model(partition_key)]
    /// #     owner_id: String,
    /// #     ciphertext: String,
    /// # }
    /// # fn example(pool: CqlPool, zone: String, owner_id: String) {
    /// let _ = pool.query::<VaultRow>().partition((zone, owner_id));
    /// # }
    /// ```
    ///
    /// [`PartitionKey`]: fundamental_core::model::CqlModel::PartitionKey
    pub fn partition(self, key: M::PartitionKey) -> CqlQuery<M, HasPartitionKey>
    where
        M::PartitionKey: PartitionValues,
    {
        let values = key.to_partition_values();
        self.bind_partition(values)
    }

    /// Specifies the partition key value, for a model partitioned by **one** column.
    ///
    /// Bounded on [`SinglePartitionModel`], so a composite partition goes through
    /// [`partition`](Self::partition).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fundamental_cql::{CqlValue, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     payload: String,
    /// # }
    /// # fn example(pool: CqlPool, tenant_id: uuid::Uuid) {
    /// let _ = pool.query::<Event>().partition_key(CqlValue::uuid(tenant_id));
    /// # }
    /// ```
    pub fn partition_key(self, value: CqlValue) -> CqlQuery<M, HasPartitionKey>
    where
        M: SinglePartitionModel,
    {
        self.bind_partition(vec![value])
    }

    /// Addresses a partition by hand-built [`CqlValue`]s — the **unchecked** door.
    ///
    /// Neither arity nor order is checked; for a projection whose partition columns are known only
    /// at run time.
    pub fn partition_key_composite<I>(self, values: I) -> CqlQuery<M, HasPartitionKey>
    where
        I: IntoIterator<Item = CqlValue>,
    {
        self.bind_partition(values.into_iter().collect())
    }

    /// The one place the partition values are stored and the typestate advances.
    fn bind_partition(mut self, values: Vec<CqlValue>) -> CqlQuery<M, HasPartitionKey> {
        self.partition_key = values;
        CqlQuery {
            session: self.session,
            cache: self.cache,
            keyspace: self.keyspace,
            key_manager: self.key_manager,
            partition_key: self.partition_key,
            clustering_filters: self.clustering_filters,
            filters: self.filters,
            select_columns: self.select_columns,
            limit: self.limit,
            consistency: self.consistency,
            allow_filtering: self.allow_filtering,
            timeout: self.timeout,
            _model: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<M, State> CqlQuery<M, State>
where
    M: BoundTo<CqlDb> + CqlModel,
{
    /// Adds an equality filter on a clustering column.
    pub fn clustering_eq(mut self, column: &str, value: CqlValue) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Eq(value),
        });
        self
    }

    /// Adds a greater-than filter on a clustering column.
    pub fn clustering_gt(mut self, column: &str, value: CqlValue) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Gt(value),
        });
        self
    }

    /// Adds a greater-than-or-equal filter on a clustering column.
    pub fn clustering_gte(mut self, column: &str, value: CqlValue) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Gte(value),
        });
        self
    }

    /// Adds a less-than filter on a clustering column.
    pub fn clustering_lt(mut self, column: &str, value: CqlValue) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Lt(value),
        });
        self
    }

    /// Adds a less-than-or-equal filter on a clustering column.
    pub fn clustering_lte(mut self, column: &str, value: CqlValue) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Lte(value),
        });
        self
    }

    /// Adds a range filter on a clustering column.
    pub fn clustering_range(
        mut self,
        column: &str,
        low: Bound<CqlValue>,
        high: Bound<CqlValue>,
    ) -> Self {
        self.clustering_filters.push(ClusteringFilter {
            column: column.to_string(),
            op: ClusteringOp::Range { low, high },
        });
        self
    }

    /// Specifies columns to select.
    pub fn select<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.select_columns = Some(columns.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the maximum number of rows to return.
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }

    /// Sets the consistency level.
    pub fn consistency(mut self, consistency: Consistency) -> Self {
        self.consistency = consistency;
        self
    }

    /// Enables ALLOW FILTERING.
    ///
    /// It may scan large amounts of data.
    pub fn allow_filtering(mut self) -> Self {
        self.allow_filtering = true;
        self
    }

    /// Sets a timeout for this specific query.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use std::time::Duration;
    /// use fundamental_cql::{CqlValue, CqlPool};
    /// # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
    /// # #[model(cql, table = "events")]
    /// # struct Event {
    /// #     #[model(partition_key)]
    /// #     tenant_id: uuid::Uuid,
    /// #     payload: String,
    /// # }
    /// # async fn example(pool: CqlPool, user_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    /// let results = pool.query::<Event>()
    ///     .partition_key(CqlValue::uuid(user_id))
    ///     .with_timeout(Duration::from_secs(5))
    ///     .execute()
    ///     .await?;
    /// # let _ = results;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl<M> CqlQuery<M, HasPartitionKey>
where
    M: BoundTo<CqlDb> + CqlModel + FromRow + ToRow + Send + Sync,
{
    /// Executes the query and returns all matching rows.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(self) -> DatabaseResult<Vec<M>> {
        if let Some(timeout) = self.timeout {
            match tokio::time::timeout(timeout, self.execute_inner()).await {
                Ok(result) => result,
                Err(_) => Err(DatabaseError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                }),
            }
        } else {
            self.execute_inner().await
        }
    }

    /// Internal execute method without timeout wrapper.
    async fn execute_inner(self) -> DatabaseResult<Vec<M>> {
        let (cql, values) = self.build_cql()?;

        let stmt = self.cache.get_or_prepare(&cql).await?;

        // The cache shares the statement, so clone it before setting per-query consistency.
        let mut prepared = (*stmt).clone();
        prepared.set_consistency(self.consistency.to_driver());

        let cql_values = CqlValues(values);

        let result = self
            .session
            .inner()
            .execute_unpaged(&prepared, &cql_values)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        // A response that is not rows is a decode fault, not an empty result.
        let rows_result = result.into_rows_result().map_err(|e| {
            warn!(table = M::TABLE, error = %e, "query did not return a rows result");
            DatabaseError::query(format!(
                "{}: expected rows result for SELECT: {e}",
                M::TABLE
            ))
        })?;
        let rows = rows_result.rows::<scylla::value::Row>().map_err(|e| {
            warn!(table = M::TABLE, error = %e, "failed to decode query rows");
            DatabaseError::serialization(format!("{}: failed to decode rows: {e}", M::TABLE))
        })?;
        let key_positions = self.key_positions();
        let mut keys = Vec::new();
        let mut models = Vec::new();
        for row_result in rows {
            let row = row_result.map_err(|e| DatabaseError::query(e.to_string()))?;
            if let Some(positions) = &key_positions {
                keys.push(crate::translations::WireKey(
                    positions
                        .iter()
                        .map(|&i| row.columns.get(i).cloned().flatten())
                        .collect(),
                ));
            }
            let model = M::from_row_encrypted(row, self.key_manager.as_deref())
                .map_err(|e| DatabaseError::serialization(e.to_string()))?;
            models.push(model);
        }

        // Translations for the whole page in one read, matched on the clustering values; a
        // projection without them is read row by row.
        if key_positions.is_some() {
            models = crate::translations::hydrate_page(
                &self.session,
                &self.cache,
                &self.keyspace,
                &self.partition_key,
                keys.into_iter().zip(models).collect(),
            )
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        } else {
            for model in &mut models {
                crate::translations::hydrate(&self.session, &self.cache, &self.keyspace, model)
                    .await
                    .map_err(|e| DatabaseError::query(e.to_string()))?;
            }
        }

        Ok(models)
    }

    /// Where each clustering column sits in the page's rows, for a model with translations
    /// whose projection selects them all; `None` otherwise.
    fn key_positions(&self) -> Option<Vec<usize>> {
        M::translation_table()?;
        let default_cols = M::columns();
        let position = |column: &str| match self.select_columns.as_deref() {
            Some(selected) => selected.iter().position(|c| c == column),
            None => default_cols.iter().position(|c| *c == column),
        };
        <M as ToRow>::clustering_key_columns()
            .iter()
            .map(|column| position(column))
            .collect()
    }

    /// Executes the query and returns the first matching row.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute_one(self) -> DatabaseResult<Option<M>> {
        let query = self.limit(1);
        let mut results = query.execute().await?;
        Ok(results.pop())
    }

    /// Builds the CQL query string and values.
    ///
    /// # Errors
    ///
    /// Returns [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier)
    /// (surfaced as a [`DatabaseError`]) if a selected or clustering column is
    /// not a safe identifier.
    fn build_cql(&self) -> CqlResult<(String, Vec<CqlValue>)> {
        let pk_cols = <M as ToRow>::partition_key_columns();
        // The model's columns in field order rather than `*`: YCQL sorts `SELECT *`'s regular
        // columns alphabetically, and `from_row` reads positionally.
        let default_cols: Vec<String>;
        let projection: Option<&[String]> = match self.select_columns.as_deref() {
            Some(cols) => Some(cols),
            None => {
                default_cols = M::columns().iter().map(|c| (*c).to_string()).collect();
                Some(&default_cols)
            }
        };
        build_select_cql(
            crate::row::qualify(&self.keyspace, M::TABLE),
            projection,
            pk_cols,
            &self.partition_key,
            &self.clustering_filters,
            self.limit,
            self.allow_filtering,
        )
    }
}
