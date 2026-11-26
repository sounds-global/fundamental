//! Shared, executor-generic CRUD core for YSQL: every statement is built and bound once, for pool and transaction alike.

use sqlx::Postgres;
use tracing::debug;

use crate::ident::{validate_conflict_target, validate_identifier};
use crate::model::key_predicate::key_predicate;
use crate::model::translations::{
    bind_companion, decode, select_from, stored_row, whole_rows_only, with_companion_delete,
    with_companion_write,
};
use crate::query::BindKey;
use crate::query::RowLock;
use crate::query::SqlQuery;
use crate::query::pg_query;
use crate::{
    DeleteBuilder, FromRow, InsertBuilder, SqlError, SqlResult, ToRow, UpdateBuilder,
    UpsertBuilder, bind_filter_value, build_count_sql, build_delete_sql, build_select_sql,
    build_update_sql, build_where_clause,
};
use fundamental_core::Model;
use fundamental_core::model::{BoundTo, SqlDb, SqlModel};
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};
use fundamental_encryption::KeyManager;

/// The ` AND <col> IS NULL` fragment for a soft-deletable model, or an empty string.
pub(crate) fn soft_delete_and<M: SqlModel>() -> String {
    if M::SOFT_DELETE {
        format!(" AND {} IS NULL", M::DELETED_AT_COLUMN)
    } else {
        String::new()
    }
}

/// Runs `delete` and returns how many rows it deleted, with a translatable model's companion rows in the same statement.
async fn run_delete<'e, 'q, E, M>(
    exec: E,
    schema: &SchemaName,
    delete: String,
    bind: impl FnOnce(
        sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel,
{
    match with_companion_delete::<M>(schema, &delete) {
        Some(counted) => {
            let row = bind(pg_query(counted))
                .fetch_one(exec)
                .await
                .map_err(SqlError::from)?;
            let deleted: i64 = sqlx::Row::get(&row, 0);
            Ok(deleted.unsigned_abs())
        }
        None => Ok(bind(pg_query(delete))
            .execute(exec)
            .await
            .map_err(SqlError::from)?
            .rows_affected()),
    }
}

/// Finds a model by its primary key.
pub(crate) async fn find_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    id: &M::Id,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!(
        "{} WHERE {} = $1{}",
        select_from::<M>(schema),
        M::PRIMARY_KEY,
        soft_delete_and::<M>(),
    );

    let row = pg_query(sql)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(SqlError::from)?;

    match row {
        Some(row) => Ok(Some(decode::<M>(&row, enc)?)),
        None => Ok(None),
    }
}

/// Finds a model by its whole composite primary key, decrypting through `enc`.
pub(crate) async fn find_by_key_with<'e, 'k, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    key: &'k M::KeyValues,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M: fundamental_core::model::CompositeKeyModel,
    M::KeyValues: BindKey<'k>,
{
    let sql = format!(
        "{} WHERE {}{} LIMIT 1",
        select_from::<M>(schema),
        key_predicate::<M>().map_err(|error| SqlError::query(error.to_string()))?,
        soft_delete_and::<M>(),
    );

    let row = key
        .bind_all(pg_query(sql))
        .fetch_optional(exec)
        .await
        .map_err(SqlError::from)?;

    match row {
        Some(row) => Ok(Some(decode::<M>(&row, enc)?)),
        None => Ok(None),
    }
}

/// The most rows one [`find_many_for_update_with`] locks.
pub(crate) const MAX_LOCKED_ROWS: usize = 1_000;

/// Finds the models whose primary keys are in `ids` and locks each, sorted here so overlapping callers cannot deadlock.
pub(crate) async fn find_many_for_update_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    ids: &[M::Id],
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Ord + Clone + Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let mut keys = ids.to_vec();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    if keys.len() > MAX_LOCKED_ROWS {
        return Err(SqlError::Query(format!(
            "refusing to lock {} rows of {} in one statement; the bound is {MAX_LOCKED_ROWS}",
            keys.len(),
            M::TABLE,
        )));
    }

    let placeholders = (1..=keys.len())
        .map(|position| format!("${position}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "{} WHERE {} IN ({placeholders}){} ORDER BY {} FOR UPDATE",
        select_from::<M>(schema),
        M::PRIMARY_KEY,
        soft_delete_and::<M>(),
        M::PRIMARY_KEY,
    );

    let mut query = pg_query(sql);
    for key in &keys {
        query = query.bind(key);
    }
    let rows = query.fetch_all(exec).await.map_err(SqlError::from)?;

    let mut models = Vec::with_capacity(rows.len());
    for row in &rows {
        models.push(decode::<M>(row, enc)?);
    }
    Ok(models)
}

/// Finds a model by its primary key and takes a row lock for the rest of the enclosing transaction.
pub(crate) async fn find_for_update_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    id: &M::Id,
    lock: RowLock,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!(
        "{} WHERE {} = $1{} {}",
        select_from::<M>(schema),
        M::PRIMARY_KEY,
        soft_delete_and::<M>(),
        lock.to_sql(),
    );

    let row = pg_query(sql)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(SqlError::from)?;

    match row {
        Some(row) => Ok(Some(decode::<M>(&row, enc)?)),
        None => Ok(None),
    }
}

/// Checks whether a row with the given primary key exists.
pub(crate) async fn exists_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    id: &M::Id,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE {} = $1{})",
        schema.qualify(M::TABLE),
        M::PRIMARY_KEY,
        soft_delete_and::<M>(),
    );

    let row = pg_query(sql)
        .bind(id)
        .fetch_one(exec)
        .await
        .map_err(SqlError::from)?;

    Ok(sqlx::Row::get(&row, 0))
}

/// Inserts a single model, returning the stored row.
pub(crate) async fn insert_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
) -> SqlResult<M>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let write = InsertBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .returning("*")
        .build();
    let sql = with_companion_write::<M>(schema, &write, M::columns().len() + 1, false);

    let query = bind_companion(model.bind_to_encrypted(pg_query(sql), enc)?, model);
    let row = query.fetch_one(exec).await.map_err(SqlError::from)?;
    stored_row(&row, model, enc)
}

/// Updates a single model by primary key, returning the stored row.
pub(crate) async fn update_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
) -> SqlResult<M>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    // No `M::Id` bound: the row is addressed by `KEY_COLUMNS` through `bind_key`.
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let write = UpdateBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .where_pk(M::KEY_COLUMNS)
        .returning("*")
        .build();
    let first = M::columns_without_pk().len() + M::KEY_COLUMNS.len() + 1;
    let sql = with_companion_write::<M>(schema, &write, first, true);

    let query = model.bind_without_pk_encrypted(pg_query(sql), enc)?;
    // Every key column, after the SET list: the leading column alone would update every row sharing it.
    let query = bind_companion(model.bind_key_encrypted(query, enc)?, model);
    let row = query.fetch_one(exec).await.map_err(SqlError::from)?;
    stored_row(&row, model, enc)
}

/// Updates a single model by primary key only while the stored row still matches `fence`, in one statement.
pub(crate) async fn update_fenced_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
    fence: &QueryBuilder<M, SqlDb, State>,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    whole_rows_only::<M>("update_fenced")?;
    let mut sql = UpdateBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .where_pk(M::KEY_COLUMNS)
        .build();
    let first_fence_param = M::columns_without_pk().len() + M::KEY_COLUMNS.len() + 1;
    let (fence_clause, fence_values, _) =
        build_where_clause(fence.get_filters(), first_fence_param)?;
    if !fence_clause.is_empty() {
        sql.push_str(" AND (");
        sql.push_str(&fence_clause);
        sql.push(')');
    }
    sql.push_str(" RETURNING *");

    let query = model.bind_without_pk_encrypted(pg_query(sql), enc)?;
    let mut query = model.bind_key_encrypted(query, enc)?;
    for value in fence_values {
        query = bind_filter_value(query, value);
    }
    let row = query.fetch_optional(exec).await.map_err(SqlError::from)?;
    row.map(|row| M::from_row_encrypted(&row, enc)).transpose()
}

/// Inserts or updates a single model (upsert), returning the stored row.
pub(crate) async fn upsert_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
) -> SqlResult<M>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let write = UpsertBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .on_conflict(M::KEY_COLUMNS)
        .returning("*")
        .build::<M>();
    let sql = with_companion_write::<M>(schema, &write, M::columns().len() + 1, true);

    let query = bind_companion(model.bind_to_encrypted(pg_query(sql), enc)?, model);
    let row = query.fetch_one(exec).await.map_err(SqlError::from)?;
    stored_row(&row, model, enc)
}

/// Inserts a model, ignoring the write when it conflicts on `conflict_target`; `None` when a row already existed.
pub(crate) async fn insert_or_ignore_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
    conflict_target: &[&str],
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    validate_conflict_target(conflict_target)?;
    let write = UpsertBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .on_conflict(conflict_target)
        .do_nothing()
        .returning("*")
        .build::<M>();
    let sql = with_companion_write::<M>(schema, &write, M::columns().len() + 1, false);

    let query = bind_companion(model.bind_to_encrypted(pg_query(sql), enc)?, model);
    match query.fetch_optional(exec).await.map_err(SqlError::from)? {
        Some(row) => Ok(Some(stored_row(&row, model, enc)?)),
        None => Ok(None),
    }
}

/// Inserts a model, or locks and returns the row already stored under its key, in one statement.
pub(crate) async fn insert_or_lock_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
) -> SqlResult<M>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    whole_rows_only::<M>("insert_or_lock")?;
    validate_conflict_target(M::KEY_COLUMNS)?;
    let sql = UpsertBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .on_conflict(M::KEY_COLUMNS)
        .keep_stored(M::TABLE)
        .returning("*")
        .build::<M>();

    let query = model.bind_to_encrypted(pg_query(sql), enc)?;
    let row = query.fetch_one(exec).await.map_err(SqlError::from)?;
    M::from_row_encrypted(&row, enc)
}

/// Upserts a model, overwriting an existing row only when `guard` holds: a monotonic compare-and-set.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) async fn upsert_guarded_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
    conflict_target: &[&str],
    guard: &crate::ColumnGuard,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    whole_rows_only::<M>("upsert_guarded")?;
    validate_conflict_target(conflict_target)?;
    validate_identifier(guard.column(), false)?;
    // `DO UPDATE` from `EXCLUDED` would move an envelope sealed to one key onto a row addressed by another.
    if !M::secret_columns().is_empty() && conflict_target != M::KEY_COLUMNS {
        return Err(SqlError::Config(format!(
            "{} has encrypted columns, so a guarded upsert must conflict on its key {:?}, not {conflict_target:?}",
            M::TYPE_NAME,
            M::KEY_COLUMNS
        )));
    }
    let predicate = format!(
        "excluded.{col} {op} {table}.{col}",
        col = guard.column(),
        op = guard.operator(),
        table = M::TABLE,
    );
    let sql = UpsertBuilder::new(schema.qualify(M::TABLE))
        .columns_from::<M>()
        .on_conflict(conflict_target)
        .guard(predicate)
        .returning("*")
        .build::<M>();

    let query = model.bind_to_encrypted(pg_query(sql), enc)?;
    match query.fetch_optional(exec).await.map_err(SqlError::from)? {
        Some(row) => Ok(Some(M::from_row_encrypted(&row, enc)?)),
        None => Ok(None),
    }
}

/// Idempotently appends a row keyed by a unique column, assigning its primary key from a
/// gap-free counter consumed only when a row is inserted.
#[expect(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "readability baseline"
)]
pub(crate) async fn insert_or_get_sequenced_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    model: &M,
    unique_column: &str,
    sequence: &crate::SequenceRef<'_>,
) -> SqlResult<(M, crate::WriteOutcome)>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    whole_rows_only::<M>("insert_or_get_sequenced")?;
    validate_identifier(unique_column, false)?;
    sequence.validate()?;
    // The database assigns the key after binding, so nothing could seal an encrypted value to its row.
    if !M::secret_columns().is_empty() {
        return Err(SqlError::Config(format!(
            "{} has encrypted columns, which a sequence-assigned key cannot bind; key it by a value the model holds",
            M::TYPE_NAME
        )));
    }

    let pk = M::PRIMARY_KEY;
    let non_pk = M::columns_without_pk();
    let unique_idx = non_pk
        .iter()
        .position(|c| *c == unique_column)
        .ok_or_else(|| {
            SqlError::query(format!(
                "unique column '{unique_column}' is not a non-primary-key column of {}",
                M::TYPE_NAME
            ))
        })?;

    // $1 is the dedupe value, $2 the sequence key, $3.. the other columns in order.
    let mut param = 3;
    let select_list: Vec<String> = M::columns()
        .iter()
        .map(|col| {
            if *col == pk {
                "__seqval".to_string()
            } else {
                let placeholder = format!("${param}");
                param += 1;
                placeholder
            }
        })
        .collect();

    let table = schema.qualify(M::TABLE);
    let sql = format!(
        "WITH existing AS ( \
             SELECT * FROM {table} WHERE {unique_column} = $1 \
         ), seq AS ( \
             UPDATE {seq_table} SET {next} = {next} + 1 \
             WHERE {key_col} = $2 AND NOT EXISTS (SELECT 1 FROM existing) \
             RETURNING {next} - 1 AS __seqval \
         ), inserted AS ( \
             INSERT INTO {table} ({insert_cols}) \
             SELECT {select_list} FROM seq \
             RETURNING * \
         ) \
         SELECT *, false AS __inserted FROM existing \
         UNION ALL \
         SELECT *, true AS __inserted FROM inserted",
        insert_cols = M::columns().join(", "),
        // Qualified like the model table, so both rows are in one namespace.
        seq_table = schema.qualify(sequence.table),
        next = sequence.next_column,
        key_col = sequence.key_column,
        select_list = select_list.join(", "),
    );

    let mut query = model.bind_column_for_batch_encrypted(pg_query(sql), unique_idx, enc)?;
    query = query.bind(sequence.key);
    query = model.bind_without_pk_encrypted(query, enc)?;

    let row = query.fetch_one(exec).await.map_err(SqlError::from)?;
    let inserted: bool = sqlx::Row::try_get(&row, "__inserted").map_err(SqlError::from)?;
    let stored = M::from_row_encrypted(&row, enc)?;
    let outcome = if inserted {
        crate::WriteOutcome::Inserted
    } else {
        crate::WriteOutcome::Existing
    };
    Ok((stored, outcome))
}

/// Deletes a single model by primary key. Returns whether a row was removed.
pub(crate) async fn delete_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    id: &M::Id,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = DeleteBuilder::new(schema.qualify(M::TABLE))
        .where_pk(&[M::PRIMARY_KEY])
        .build();

    Ok(run_delete::<_, M>(exec, schema, sql, |query| query.bind(id)).await? > 0)
}

/// Deletes a row by its whole composite primary key.
pub(crate) async fn delete_by_key_with<'e, 'k, E, M>(
    exec: E,
    schema: &SchemaName,
    key: &'k M::KeyValues,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M: fundamental_core::model::CompositeKeyModel,
    M::KeyValues: BindKey<'k>,
{
    let sql = format!(
        "DELETE FROM {} WHERE {}",
        schema.qualify(M::TABLE),
        key_predicate::<M>().map_err(|error| SqlError::query(error.to_string()))?,
    );

    Ok(run_delete::<_, M>(exec, schema, sql, |query| key.bind_all(query)).await? > 0)
}

/// Soft-deletes a row by its whole composite key while it is live.
pub(crate) async fn soft_delete_by_key_with<'e, 'k, E, M>(
    exec: E,
    schema: &SchemaName,
    key: &'k M::KeyValues,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel + fundamental_core::model::CompositeKeyModel,
    M::KeyValues: BindKey<'k>,
{
    set_deleted_by_key_with::<E, M>(exec, schema, key, "now()", "IS NULL").await
}

/// Restores a soft-deleted row by its whole composite key while it is deleted.
pub(crate) async fn restore_by_key_with<'e, 'k, E, M>(
    exec: E,
    schema: &SchemaName,
    key: &'k M::KeyValues,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel + fundamental_core::model::CompositeKeyModel,
    M::KeyValues: BindKey<'k>,
{
    set_deleted_by_key_with::<E, M>(exec, schema, key, "NULL", "IS NOT NULL").await
}

/// The one statement both by-key soft-delete directions run; its literals come from the two callers.
async fn set_deleted_by_key_with<'e, 'k, E, M>(
    exec: E,
    schema: &SchemaName,
    key: &'k M::KeyValues,
    value: &'static str,
    state: &'static str,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel + fundamental_core::model::CompositeKeyModel,
    M::KeyValues: BindKey<'k>,
{
    if !M::SOFT_DELETE {
        return Err(SqlError::query(format!(
            "`{}` is not declared `#[model(soft_delete)]`, so it has no `{}` to set: delete it \
             by key, or declare it soft-deletable",
            M::TABLE,
            M::DELETED_AT_COLUMN,
        )));
    }
    let sql = format!(
        "UPDATE {} SET {} = {value} WHERE {} AND {} {state}",
        schema.qualify(M::TABLE),
        M::DELETED_AT_COLUMN,
        key_predicate::<M>().map_err(|error| SqlError::query(error.to_string()))?,
        M::DELETED_AT_COLUMN,
    );
    let result = key
        .bind_all(pg_query(sql))
        .execute(exec)
        .await
        .map_err(SqlError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Finds multiple models by primary key (IN clause). Missing IDs are skipped.
pub(crate) async fn find_many_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    ids: &[M::Id],
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    if ids.is_empty() {
        debug!("find_many called with no ids; nothing to do");
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("${}", i)).collect();
    let sql = format!(
        "{} WHERE {} IN ({}){}",
        select_from::<M>(schema),
        M::PRIMARY_KEY,
        placeholders.join(", "),
        soft_delete_and::<M>(),
    );

    let mut query = pg_query(sql);
    for id in ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter().map(|row| decode::<M>(row, enc)).collect()
}

/// Deletes multiple models by primary key (IN clause). Returns the row count.
pub(crate) async fn delete_many_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    ids: &[M::Id],
) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    if ids.is_empty() {
        debug!("delete_many called with no ids; nothing to do");
        return Ok(0);
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("${}", i)).collect();
    let sql = format!(
        "DELETE FROM {} WHERE {} IN ({})",
        schema.qualify(M::TABLE),
        M::PRIMARY_KEY,
        placeholders.join(", "),
    );

    run_delete::<_, M>(exec, schema, sql, |mut query| {
        for id in ids {
            query = query.bind(id);
        }
        query
    })
    .await
}

/// Inserts a chunk of models with a single multi-row `INSERT ... RETURNING *`; the caller chunks.
pub(crate) async fn insert_chunk_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    models: &[M],
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    whole_rows_only::<M>("insert_many")?;
    if models.is_empty() {
        debug!("insert_chunk called with no models; nothing to do");
        return Ok(Vec::new());
    }

    let columns = M::columns();
    let num_cols = columns.len();

    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES ",
        schema.qualify(M::TABLE),
        columns.join(", ")
    );

    let mut value_groups = Vec::with_capacity(models.len());
    let mut param_idx = 1;
    for _ in models {
        let placeholders: Vec<String> = (0..num_cols)
            .map(|_| {
                let p = format!("${}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        value_groups.push(format!("({})", placeholders.join(", ")));
    }

    sql.push_str(&value_groups.join(", "));
    sql.push_str(" RETURNING *");

    let mut query = pg_query(sql);
    for model in models {
        query = model.bind_to_encrypted(query, enc)?;
    }

    let rows = query.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter()
        .map(|row| M::from_row_encrypted(row, enc))
        .collect()
}

/// Updates a chunk of models with a single `UPDATE ... CASE`, addressing rows by their whole key; the caller chunks.
///
/// Each `CASE` carries an `ELSE` naming its own column, so an unmatched row is never set to `NULL`.
pub(crate) async fn update_chunk_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    models: &[M],
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    // No `M::Id` bound: rows are addressed by `KEY_COLUMNS` through `bind_key`.
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    if models.is_empty() {
        debug!("update_chunk called with no models; nothing to do");
        return Ok(Vec::new());
    }

    let columns = M::columns_without_pk();
    let key_columns = M::KEY_COLUMNS;
    let table = schema.qualify(M::TABLE);
    let num_models = models.len();
    let cols_per_model = columns.len();

    for column in key_columns {
        validate_identifier(column, false)?;
    }
    let key_tuple = format!("({})", key_columns.join(", "));

    let mut sql = format!("UPDATE {} SET ", table);
    let mut param_idx = 1;

    let key_tuples: Vec<String> = (0..num_models)
        .map(|model_idx| {
            let placeholders: Vec<String> = (0..key_columns.len())
                .map(|at| format!("${}", param_idx + model_idx * key_columns.len() + at))
                .collect();
            format!("({})", placeholders.join(", "))
        })
        .collect();
    param_idx += num_models * key_columns.len();

    let mut set_clauses = Vec::with_capacity(columns.len());
    for col in columns.iter() {
        let mut case_expr = format!("{} = CASE ", col);
        for key_tuple_placeholders in &key_tuples {
            let val_placeholder = format!("${}", param_idx);
            param_idx += 1;
            case_expr.push_str(&format!(
                "WHEN {} = {} THEN {} ",
                key_tuple, key_tuple_placeholders, val_placeholder
            ));
        }
        case_expr.push_str(&format!("ELSE {} END", col));
        set_clauses.push(case_expr);
    }

    sql.push_str(&set_clauses.join(", "));
    sql.push_str(&format!(
        " WHERE {} IN ({})",
        key_tuple,
        key_tuples.join(", ")
    ));
    sql.push_str(" RETURNING *");

    let mut query = pg_query(sql);
    for model in models {
        query = model.bind_key_encrypted(query, enc)?;
    }
    for col_idx in 0..cols_per_model {
        for model in models {
            query = model.bind_column_for_batch_encrypted(query, col_idx, enc)?;
        }
    }

    let rows = query.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter()
        .map(|row| M::from_row_encrypted(row, enc))
        .collect()
}

/// Counts all rows, excluding soft-deleted ones.
pub(crate) async fn count_with<'e, E, M>(exec: E, schema: &SchemaName) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let where_clause = if M::SOFT_DELETE {
        format!(" WHERE {} IS NULL", M::DELETED_AT_COLUMN)
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT COUNT(*) FROM {}{}",
        schema.qualify(M::TABLE),
        where_clause
    );

    let row = pg_query(sql)
        .fetch_one(exec)
        .await
        .map_err(SqlError::from)?;
    let count: i64 = sqlx::Row::get(&row, 0);
    Ok(count as u64)
}

/// Counts rows matching a [`QueryBuilder`], reusing the shared count-SQL builder.
pub(crate) async fn count_where_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) = build_count_sql::<M, _>(schema, query)?;
    let mut q = pg_query(sql);
    for value in values {
        q = bind_filter_value(q, value);
    }

    let row = q.fetch_one(exec).await.map_err(SqlError::from)?;
    let count: i64 = sqlx::Row::get(&row, 0);
    Ok(count as u64)
}

/// Runs a [`SqlQuery`], which may carry a row lock, and decodes the rows, decrypting through `enc`.
pub(crate) async fn select_locked_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    query: &SqlQuery<M, State>,
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) = build_select_sql::<M, _>(schema, query.inner(), query.get_row_lock())?;
    let mut q = pg_query(sql);
    for value in values {
        q = bind_filter_value(q, value);
    }

    let rows = q.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter().map(|row| decode::<M>(row, enc)).collect()
}

pub(crate) async fn select_where_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    query: &QueryBuilder<M, SqlDb, State>,
) -> SqlResult<Vec<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) = build_select_sql::<M, _>(schema, query, None)?;
    let mut q = pg_query(sql);
    for value in values {
        q = bind_filter_value(q, value);
    }

    let rows = q.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter().map(|row| decode::<M>(row, enc)).collect()
}

/// Deletes rows matching a [`QueryBuilder`], returning the affected row count.
pub(crate) async fn delete_where_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) = build_delete_sql::<M, _>(schema, query)?;
    run_delete::<_, M>(exec, schema, sql, |mut q| {
        for value in values {
            q = bind_filter_value(q, value);
        }
        q
    })
    .await
}

/// Updates rows matching a [`QueryBuilder`], setting each `(column, value)`; returns the affected row count.
pub(crate) async fn update_where_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    updates: &[(String, FilterValue)],
) -> SqlResult<u64>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) = build_update_sql::<M, _>(schema, query, updates)?;
    let mut q = pg_query(sql);
    for value in values {
        q = bind_filter_value(q, value);
    }

    let result = q.execute(exec).await.map_err(SqlError::from)?;
    Ok(result.rows_affected())
}

/// Soft-deletes a row only if not already soft-deleted, so the result says whether this call deleted it.
pub(crate) async fn soft_delete_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    id: &M::Id,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!(
        "UPDATE {} SET {} = now() WHERE {} = $1 AND {} IS NULL",
        schema.qualify(M::TABLE),
        M::DELETED_AT_COLUMN,
        M::PRIMARY_KEY,
        M::DELETED_AT_COLUMN,
    );

    let result = pg_query(sql)
        .bind(id)
        .execute(exec)
        .await
        .map_err(SqlError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Restores a soft-deleted row, clearing `deleted_at`.
pub(crate) async fn restore_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    id: &M::Id,
) -> SqlResult<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!(
        "UPDATE {} SET {} = NULL WHERE {} = $1",
        schema.qualify(M::TABLE),
        M::DELETED_AT_COLUMN,
        M::PRIMARY_KEY,
    );

    let result = pg_query(sql)
        .bind(id)
        .execute(exec)
        .await
        .map_err(SqlError::from)?;
    Ok(result.rows_affected() > 0)
}

/// Finds a row by primary key including soft-deleted rows (no soft-delete filter).
pub(crate) async fn find_with_deleted_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    id: &M::Id,
) -> SqlResult<Option<M>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    let sql = format!("{} WHERE {} = $1", select_from::<M>(schema), M::PRIMARY_KEY,);

    let row = pg_query(sql)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(SqlError::from)?;

    match row {
        Some(row) => Ok(Some(decode::<M>(&row, enc)?)),
        None => Ok(None),
    }
}
