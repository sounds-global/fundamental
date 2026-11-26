//! Abstract repository trait implementations for [`CqlRepository`].
//!
//! YCQL keys are composite, so the abstract `Repository`'s `Id` is [`ToRow::KeyValues`], the
//! whole key as a tuple.

use fundamental_core::Model;
use fundamental_core::model::{BoundTo, CqlDb, CqlModel, LeadingKeyAddressed, Satisfied};
use fundamental_db::{
    DatabaseError, DatabaseResult, KeyedRepository, KeyedRepositoryBatch, QueryBuilder, Repository,
    RepositoryAggregate, RepositoryBatch, RepositoryExt, RepositorySoftDelete,
};
use scylla::serialize::row::SerializeRow;
use tracing::{debug, instrument, warn};

use crate::{
    BatchType, CqlValues, FromRow, ToRow, build_count_cql, build_count_predicate, build_select_cql,
};

use super::CqlRepository;

impl<M> CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
    M::KeyValues: SerializeRow + Send + Sync,
{
    /// The ` pk = ? AND ck = ? ...` WHERE clause, in [`ToRow::KeyValues`] order.
    fn key_where_clause() -> String {
        let mut cols: Vec<&'static str> = M::partition_key_columns().to_vec();
        cols.extend_from_slice(M::clustering_key_columns());
        cols.iter()
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// Fetches a single row by its composite primary key, hydrating translations and skipping a
    /// soft-deleted row.
    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME))]
    async fn find_by_key(&self, id: &M::KeyValues) -> DatabaseResult<Option<M>> {
        self.find_by_key_filtered(id, M::SOFT_DELETE).await
    }

    /// Fetches a single row by its composite primary key, optionally applying the
    /// soft-delete filter.
    ///
    /// CQL cannot express `WHERE deleted_at IS NULL` on a key read, so the filter reads the row's
    /// `deleted_at`, located by name.
    #[instrument(
        level = "debug",
        skip(self, id),
        fields(table = M::TABLE, model = M::TYPE_NAME, filter_deleted)
    )]
    async fn find_by_key_filtered(
        &self,
        id: &M::KeyValues,
        filter_deleted: bool,
    ) -> DatabaseResult<Option<M>> {
        // YCQL returns `SELECT *` as key columns, then the rest alphabetically, so the model's
        // columns are projected in field order; `deleted_at` is appended and located by name.
        let mut projection = M::columns().join(", ");
        if M::SOFT_DELETE {
            projection.push_str(", ");
            projection.push_str(M::DELETED_AT_COLUMN);
        }
        let cql = format!(
            "SELECT {} FROM {} WHERE {}",
            projection,
            crate::row::qualify(&self.keyspace, M::TABLE),
            Self::key_where_clause(),
        );

        let stmt = self.prepared_read(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, id)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        let rows_result = match result.into_rows_result() {
            Ok(r) => r,
            // A SELECT with no rows result is a protocol fault, not "not found".
            Err(e) => {
                warn!(table = M::TABLE, model = M::TYPE_NAME, error = %e, "find: query did not return a rows result");
                return Err(DatabaseError::query(format!(
                    "{}: expected rows result for SELECT: {e}",
                    M::TABLE
                )));
            }
        };

        let deleted_at_idx = if filter_deleted {
            rows_result
                .column_specs()
                .get_by_name(M::DELETED_AT_COLUMN)
                .map(|(idx, _)| idx)
        } else {
            None
        };

        let mut rows = rows_result
            .rows::<scylla::value::Row>()
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        let row = match rows.next() {
            Some(Ok(row)) => row,
            Some(Err(e)) => return Err(DatabaseError::query(e.to_string())),
            None => return Ok(None),
        };

        if let Some(idx) = deleted_at_idx
            && matches!(row.columns.get(idx), Some(Some(_)))
        {
            return Ok(None);
        }

        let mut model = M::from_row_encrypted(row, self.key_manager.as_deref())
            .map_err(|e| DatabaseError::serialization(e.to_string()))?;

        crate::translations::hydrate(&self.session, &self.cache, &self.keyspace, &mut model)
            .await?;

        Ok(Some(model))
    }
}

impl<M> Repository<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Clone + Send + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    /// The composite primary key tuple (partition key + clustering key).
    type Id = M::KeyValues;

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn insert(&self, model: &M) -> DatabaseResult<M> {
        // An INSERT in YCQL is an upsert.
        self.timed("insert", self.exec_insert(model, None, None, false))
            .await?;
        Ok(model.clone())
    }

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn update(&self, model: &M) -> DatabaseResult<M> {
        self.timed("update", self.exec_update(model, None, None, false))
            .await?;
        Ok(model.clone())
    }

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn upsert(&self, model: &M) -> DatabaseResult<M> {
        // A YCQL INSERT is an upsert.
        self.timed("upsert", self.exec_insert(model, None, None, false))
            .await?;
        Ok(model.clone())
    }

    fn query(&self) -> QueryBuilder<M, CqlDb> {
        QueryBuilder::new()
    }
}

/// The id-taking half: YCQL's `Id` is the whole key as a tuple.
impl<M> KeyedRepository<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Clone + Send + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        self.timed("select", self.find_by_key(id)).await
    }

    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find_required(&self, id: &Self::Id) -> DatabaseResult<M> {
        self.find_by_key(id)
            .await?
            .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<composite-key>".to_string()))
    }

    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn exists(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let key_col = M::partition_key_columns().first().copied().unwrap_or("*");
        let cql = format!(
            "SELECT {} FROM {} WHERE {} LIMIT 1",
            key_col,
            crate::row::qualify(&self.keyspace, M::TABLE),
            Self::key_where_clause(),
        );

        let stmt = self.prepared_read(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, id)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        let rows_result = result.into_rows_result().map_err(|e| {
            warn!(table = M::TABLE, model = M::TYPE_NAME, error = %e, "exists: query did not return a rows result");
            DatabaseError::query(format!("{}: expected rows result for SELECT: {e}", M::TABLE))
        })?;
        let mut rows = rows_result.rows::<scylla::value::Row>().map_err(|e| {
            warn!(table = M::TABLE, model = M::TYPE_NAME, error = %e, "exists: failed to decode rows");
            DatabaseError::serialization(format!("{}: failed to decode rows: {e}", M::TABLE))
        })?;
        Ok(rows.next().is_some())
    }

    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        // YCQL does not report whether a row existed, so this returns `true`.
        self.timed("delete", self.exec_delete_keys(id, false))
            .await?;
        self.exec_delete_translations(id).await?;
        Ok(true)
    }
}

impl<M> RepositoryBatch<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Send + Sync + Clone,
    M::InsertValues: SerializeRow + Send + Sync,
{
    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn insert_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        if models.is_empty() {
            return Ok(Vec::new());
        }
        self.exec_insert_batch(models, BatchType::Logged, None)
            .await?;
        Ok(models.to_vec())
    }

    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn update_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        if models.is_empty() {
            return Ok(Vec::new());
        }
        // A YCQL INSERT is an upsert.
        self.exec_insert_batch(models, BatchType::Logged, None)
            .await?;
        Ok(models.to_vec())
    }
}

impl<M> KeyedRepositoryBatch<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Send + Sync + Clone,
    M::InsertValues: SerializeRow + Send + Sync,
{
    #[instrument(level = "debug", skip(self, ids), fields(table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn find_many(&self, ids: &[Self::Id]) -> DatabaseResult<Vec<M>> {
        // No cross-partition `IN`: one keyed SELECT per id; missing ids are skipped.
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(model) = self.find_by_key(id).await? {
                out.push(model);
            }
        }
        Ok(out)
    }

    #[instrument(skip(self, ids), fields(table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn delete_many(&self, ids: &[Self::Id]) -> DatabaseResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let count = ids.len() as u64;
        self.exec_delete_batch(ids.to_vec(), BatchType::Logged)
            .await?;
        // YCQL does not report rows affected; report the keys requested.
        Ok(count)
    }
}

impl<M> RepositoryAggregate<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Clone + Send + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    /// Counts every row in the table.
    ///
    /// A full cluster scan in YCQL; prefer partition-scoped counts.
    #[instrument(level = "debug", skip(self), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn count(&self) -> DatabaseResult<u64> {
        let cql = format!(
            "SELECT COUNT(*) FROM {}",
            crate::row::qualify(&self.keyspace, M::TABLE),
        );

        let stmt = self.prepared_read(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, &())
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        let rows_result = result
            .into_rows_result()
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        let mut rows = rows_result
            .rows::<(i64,)>()
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        match rows.next() {
            Some(Ok((count,))) => Ok(count.max(0) as u64),
            Some(Err(e)) => Err(DatabaseError::query(e.to_string())),
            None => Ok(0),
        }
    }

    /// Counts the rows matching a **partition-scoped** abstract query.
    ///
    /// The query must pin every partition-key column by equality, optionally narrowing clustering
    /// columns; an empty query counts the table. Soft-deleted rows are excluded app-side.
    ///
    /// # Errors
    ///
    /// [`DatabaseError`] (carrying `CQL_011`) when the predicate is not partition-scoped, which
    /// would need `ALLOW FILTERING`.
    #[instrument(level = "debug", skip(self, query), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    async fn count_where<State>(&self, query: &QueryBuilder<M, CqlDb, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, CqlDb, State>: Satisfied,
    {
        let filters = query.get_filters();
        if filters.is_empty() {
            debug!("count_where: empty filter, counting the whole table");
            return self.count().await;
        }

        let pred = build_count_predicate::<M>(filters)?;
        debug!(
            partition_key_cols = pred.pk_values.len(),
            clustering_filters = pred.clustering.len(),
            soft_delete = M::SOFT_DELETE,
            "count_where: partition-scoped count"
        );

        let table = crate::row::qualify(&self.keyspace, M::TABLE);

        if M::SOFT_DELETE {
            // CQL has no `IS NULL`, so only `deleted_at` is projected and the null (live) rows
            // counted.
            let projection = [M::DELETED_AT_COLUMN.to_string()];
            let (cql, values) = build_select_cql(
                table,
                Some(&projection),
                M::partition_key_columns(),
                &pred.pk_values,
                &pred.clustering,
                None,
                false,
            )?;

            let stmt = self.prepared_read(&cql).await?;
            let result = self
                .session
                .inner()
                .execute_unpaged(&stmt, &CqlValues(values))
                .await
                .map_err(|e| DatabaseError::query(e.to_string()))?;

            let rows_result = result.into_rows_result().map_err(|e| {
                warn!(table = M::TABLE, model = M::TYPE_NAME, error = %e, "count_where: query did not return a rows result");
                DatabaseError::query(format!("{}: expected rows result for COUNT: {e}", M::TABLE))
            })?;
            let rows = rows_result
                .rows::<scylla::value::Row>()
                .map_err(|e| DatabaseError::serialization(e.to_string()))?;

            let mut live = 0_u64;
            for row in rows {
                let row = row.map_err(|e| DatabaseError::query(e.to_string()))?;
                if matches!(row.columns.first(), Some(None)) {
                    live += 1;
                }
            }
            return Ok(live);
        }

        let (cql, values) = build_count_cql(
            &table,
            M::partition_key_columns(),
            &pred.pk_values,
            &pred.clustering,
        )?;

        let stmt = self.prepared_read(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, &CqlValues(values))
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        let rows_result = result
            .into_rows_result()
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        let mut rows = rows_result
            .rows::<(i64,)>()
            .map_err(|e| DatabaseError::query(e.to_string()))?;
        match rows.next() {
            Some(Ok((count,))) => Ok(count.max(0) as u64),
            Some(Err(e)) => Err(DatabaseError::query(e.to_string())),
            None => Ok(0),
        }
    }
}

impl<M> RepositoryExt<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Clone + Send + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    #[instrument(skip(self, id, create), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find_or_create<F>(&self, id: &Self::Id, create: F) -> DatabaseResult<M>
    where
        F: FnOnce() -> M + Send,
    {
        if let Some(found) = self.find_by_key(id).await? {
            return Ok(found);
        }
        let model = create();
        // Use the lightweight transaction so a concurrent creator does not get
        // clobbered; on a losing race re-read the winner's row.
        let applied = self.exec_insert(&model, None, None, true).await?;
        if applied {
            Ok(model)
        } else {
            self.find_by_key(id).await?.ok_or_else(|| {
                DatabaseError::not_found(M::TYPE_NAME, "<composite-key>".to_string())
            })
        }
    }

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn update_if_exists(&self, model: &M) -> DatabaseResult<Option<M>> {
        let applied = self.exec_update(model, None, None, true).await?;
        if applied {
            Ok(Some(model.clone()))
        } else {
            Ok(None)
        }
    }
}

impl<M> RepositorySoftDelete<M, CqlDb> for CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Model + ToRow + FromRow + Clone + Send + Sync,
    M::KeyValues: SerializeRow + Clone + Send + Sync,
    M::InsertValues: SerializeRow + Send + Sync,
{
    /// `UPDATE ks.table SET deleted_at = toTimestamp(now()) WHERE <key> IF EXISTS`.
    ///
    /// `IF EXISTS` makes it a lightweight transaction whose `[applied]` says whether a row changed.
    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn soft_delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let cql = format!(
            "UPDATE {} SET {} = toTimestamp(now()) WHERE {} IF EXISTS",
            crate::row::qualify(&self.keyspace, M::TABLE),
            M::DELETED_AT_COLUMN,
            Self::key_where_clause(),
        );

        let stmt = self.prepared_write(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, id)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        super::wrapper::parse_lwt_result(result).map_err(DatabaseError::from)
    }

    /// `UPDATE ks.table SET deleted_at = null WHERE <key> IF EXISTS`, parsing the
    /// `[applied]` flag to report whether a row existed to restore.
    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn restore(&self, id: &Self::Id) -> DatabaseResult<bool> {
        let cql = format!(
            "UPDATE {} SET {} = null WHERE {} IF EXISTS",
            crate::row::qualify(&self.keyspace, M::TABLE),
            M::DELETED_AT_COLUMN,
            Self::key_where_clause(),
        );

        let stmt = self.prepared_write(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, id)
            .await
            .map_err(|e| DatabaseError::query(e.to_string()))?;

        super::wrapper::parse_lwt_result(result).map_err(DatabaseError::from)
    }

    /// Looks up a row by key with no soft-delete filter.
    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find_with_deleted(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        self.find_by_key_filtered(id, false).await
    }

    /// Permanent hard delete by key; YCQL does not report whether a row existed, so it returns
    /// `true`.
    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn purge(&self, id: &Self::Id) -> DatabaseResult<bool> {
        self.exec_delete_batch(vec![id.clone()], BatchType::Unlogged)
            .await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_macros::Model;

    /// A YCQL model with a composite primary key, to type-check the repository traits.
    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(cql)]
    struct Event {
        #[model(partition_key)]
        tenant_id: uuid::Uuid,
        #[model(clustering_key)]
        bucket: i64,
        payload: String,
    }

    /// A soft-deletable YCQL model.
    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(cql, soft_delete)]
    struct SoftEvent {
        #[model(partition_key)]
        tenant_id: uuid::Uuid,
        #[model(clustering_key)]
        bucket: i64,
        payload: String,
    }

    #[test]
    fn soft_delete_const_and_schema() {
        // Bound to locals so clippy's `assertions_on_constants` accepts them.
        let plain_soft_delete = <Event as CqlModel>::SOFT_DELETE;
        let annotated_soft_delete = <SoftEvent as CqlModel>::SOFT_DELETE;
        assert!(
            !plain_soft_delete,
            "plain model defaults SOFT_DELETE to false"
        );
        assert!(
            annotated_soft_delete,
            "#[model(soft_delete)] sets SOFT_DELETE to true"
        );
        assert_eq!(<SoftEvent as CqlModel>::DELETED_AT_COLUMN, "deleted_at");

        // `deleted_at` is framework-managed, so it is not a serialized column.
        assert!(!SoftEvent::columns().contains(&"deleted_at"));

        let ddl = SoftEvent::create_table_sql();
        assert!(ddl.contains("deleted_at timestamp"), "DDL was: {ddl}");
        assert!(
            ddl.contains("_deleted_at_idx"),
            "expected secondary index in DDL, got: {ddl}"
        );
    }

    #[test]
    fn implements_soft_delete_trait() {
        fn assert_soft_delete<R, M>()
        where
            R: RepositorySoftDelete<M, CqlDb>,
            M: BoundTo<CqlDb> + Send + Sync,
        {
        }

        assert_soft_delete::<CqlRepository<SoftEvent>, SoftEvent>();
    }

    #[test]
    fn implements_abstract_repository_traits() {
        fn assert_repo<R, M>()
        where
            R: Repository<M, CqlDb, Id = <M as ToRow>::KeyValues>
                + RepositoryBatch<M, CqlDb>
                + RepositoryAggregate<M, CqlDb>
                + RepositoryExt<M, CqlDb>,
            M: BoundTo<CqlDb> + ToRow + Send + Sync,
        {
        }

        assert_repo::<CqlRepository<Event>, Event>();

        // The composite key tuple is exactly partition key + clustering key.
        fn assert_id_is<T>()
        where
            T: SerializeRow,
        {
        }
        assert_id_is::<<CqlRepository<Event> as Repository<Event, CqlDb>>::Id>();

        // And that the abstract query builder constructs (no I/O, just the type).
        let _builder: QueryBuilder<Event, CqlDb> = QueryBuilder::new();
    }
}
