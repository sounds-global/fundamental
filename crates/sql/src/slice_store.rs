//! Reads and writes a slice's rows in a YSQL table as sync records, using YSQL's own `::text`
//! rendering as the codec; a SQL `NULL` is the column's absence from the payload.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use sqlx::Row;

use fundamental_db::{Filter, RecordKey, SchemaName};
use fundamental_sync::wire::SliceCursor;
use fundamental_sync::{SliceStore, SliceWriteStore, SyncError, SyncMeta, SyncRecord};

use crate::SqlConnector;
use crate::ident::validate_identifier;
use crate::migration::normalize_type;
use crate::query::{build_where_clause, pg_query};
use crate::{SqlError, SqlPool, SqlResult, bind_filter_value};

type TableTypes = Arc<HashMap<String, String>>;

/// The alias the upsert gives its insert target, so `DO UPDATE SET` can name the row it updates.
const TARGET: &str = "__target";

/// Reads and writes slice rows in a YSQL pool as [`SyncRecord`]s, over a `#[model(syncable)]` table.
#[derive(Debug, Clone)]
pub struct SqlSliceStore {
    connector: SqlConnector,
    /// Taken from the [`SqlPool`], since a bare `PgPool` carries no `search_path`.
    schema: SchemaName,
    column_types: Arc<Mutex<HashMap<String, TableTypes>>>,
}

impl SqlSliceStore {
    /// Wraps a connection pool, reading the schema its statements address from it.
    #[must_use]
    pub fn new(pool: &SqlPool) -> Self {
        Self {
            connector: pool.connector().clone(),
            schema: pool.schema().clone(),
            column_types: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn qualify(&self, table: &str) -> String {
        self.schema.qualify(table)
    }

    /// Returns all rows of `table` matching `filters` whose overlay `updated_at` is after `since` (ms), unbounded.
    ///
    /// # Errors
    /// Returns a [`SqlError`] if an identifier is unsafe, the query fails,
    /// or a row cannot be decoded.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn changed_since(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
    ) -> SqlResult<Vec<SyncRecord>> {
        self.changed_page(
            table,
            data_columns,
            key_columns,
            filters,
            since,
            None,
            u32::MAX,
        )
        .await
    }

    /// Returns up to `limit` rows of `table` matching `filters`, ordered by millisecond
    /// `(updated_at, id)`, after the keyset point `after`, or after `since` when it is `None`.
    ///
    /// # Errors
    /// Returns a [`SqlError`] if an identifier is unsafe, the query fails,
    /// or a row cannot be decoded.
    #[allow(clippy::too_many_arguments)]
    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    pub async fn changed_page(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> SqlResult<Vec<SyncRecord>> {
        validate_identifier(table, false)?;
        let key_column = single_key_column(key_columns, table)?;
        validate_identifier(key_column, false)?;
        for column in data_columns {
            validate_identifier(column, false)?;
        }

        // Milliseconds, so ordering, the high-water bound and the keyset agree.
        let updated_ms = "(extract(epoch from updated_at) * 1000)::int8";

        let mut projection = vec![format!("{key_column}::text AS __id")];
        for column in data_columns {
            projection.push(format!("{column}::text AS {column}"));
        }
        projection.push(format!("{updated_ms} AS __updated_at"));
        projection.push("version AS __version".to_string());
        projection.push("_tombstone AS __tombstone".to_string());

        let key_type = match after {
            None => None,
            Some(_) => {
                let types = self.table_column_types(table).await?;
                Some(cast_type(&types, table, key_column)?.to_string())
            }
        };

        let (where_sql, values, index) = build_where_clause(filters, 1)?;
        let boundary = match &key_type {
            None => format!("{updated_ms} > ${index}"),
            // A row-value comparison with the key at its own type: as text, `'10' > '9'` is false and rows are skipped.
            Some(key_type) => format!(
                "(({updated_ms}), {key_column}) > (${index}, ${}::{key_type})",
                index + 1,
            ),
        };
        let where_full = if where_sql.is_empty() {
            boundary
        } else {
            format!("{where_sql} AND {boundary}")
        };

        let sql = format!(
            "SELECT {} FROM {} WHERE {where_full} ORDER BY {updated_ms}, {key_column} \
             LIMIT {limit}",
            projection.join(", "),
            self.qualify(table),
        );

        let mut query = pg_query(sql);
        for value in values {
            query = bind_filter_value(query, value);
        }
        match after {
            None => query = query.bind(since),
            Some(cursor) => {
                let resume = single_key(&cursor.key, table, "cursor")?.to_string();
                query = query.bind(cursor.updated_at).bind(resume);
            }
        }

        let rows = query
            .fetch_all(&mut *self.connector.acquire().await?)
            .await
            .map_err(SqlError::from)?;

        let mut records = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row.try_get("__id").map_err(SqlError::from)?;
            let updated_at: i64 = row.try_get("__updated_at").map_err(SqlError::from)?;
            let version: i64 = row.try_get("__version").map_err(SqlError::from)?;
            let tombstone: bool = row.try_get("__tombstone").map_err(SqlError::from)?;

            let mut payload = BTreeMap::new();
            for column in data_columns {
                let value: Option<String> = row.try_get(*column).map_err(SqlError::from)?;
                if let Some(value) = value {
                    payload.insert((*column).to_string(), value);
                }
            }

            records.push(SyncRecord {
                id: RecordKey::single(id),
                meta: SyncMeta {
                    updated_at,
                    version,
                    tombstone,
                },
                payload,
            });
        }
        Ok(records)
    }

    /// Applies a pushed `record` to `table` by primary key, server-stamping the sync overlay; a tombstone marks the row deleted.
    ///
    /// # Errors
    /// Returns a [`SqlError`] if an identifier is unsafe, the table or a
    /// column is missing, or the write fails.
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub async fn upsert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> SqlResult<()> {
        validate_identifier(table, false)?;
        let key_column = single_key_column(key_columns, table)?;
        validate_identifier(key_column, false)?;
        for column in data_columns {
            validate_identifier(column, false)?;
        }

        let types = self.table_column_types(table).await?;
        let pk_type = cast_type(&types, table, key_column)?;
        let id = single_key(&record.id, table, "record")?.to_string();

        // A tombstone for a row the server has never seen is a no-op.
        if record.meta.tombstone {
            let sql = format!(
                "UPDATE {} SET _tombstone = true, updated_at = now(), version = version + 1 \
                 WHERE {key_column} = $1::{pk_type}",
                self.qualify(table),
            );
            pg_query(sql)
                .bind(id.clone())
                .execute(&mut *self.connector.acquire().await?)
                .await
                .map_err(SqlError::from)?;
            return Ok(());
        }

        // The overlay is server-stamped, never taken from the client.
        let mut columns = vec![key_column.to_string()];
        let mut placeholders = vec![format!("$1::{pk_type}")];
        let mut binds: Vec<Option<String>> = vec![Some(id)];

        for (offset, column) in data_columns.iter().enumerate() {
            let column_type = cast_type(&types, table, column)?;
            columns.push((*column).to_string());
            placeholders.push(format!("${}::{column_type}", offset + 2));
            binds.push(record.payload.get(*column).cloned());
        }

        for (name, literal) in [
            ("updated_at", "now()"),
            ("version", "1"),
            ("_tombstone", "false"),
        ] {
            columns.push(name.to_string());
            placeholders.push(literal.to_string());
        }

        let mut updates: Vec<String> = data_columns
            .iter()
            .map(|column| format!("{column} = EXCLUDED.{column}"))
            .collect();
        updates.push("updated_at = now()".to_string());
        // `{TARGET}`: inside `DO UPDATE SET` the row is addressed by the insert target's alias, not a qualified name.
        updates.push(format!("version = {TARGET}.version + 1"));
        updates.push("_tombstone = false".to_string());

        let sql = format!(
            "INSERT INTO {} AS {TARGET} ({}) VALUES ({}) \
             ON CONFLICT ({key_column}) DO UPDATE SET {}",
            self.qualify(table),
            columns.join(", "),
            placeholders.join(", "),
            updates.join(", "),
        );

        let mut query = pg_query(sql);
        for value in binds {
            query = query.bind(value);
        }
        query
            .execute(&mut *self.connector.acquire().await?)
            .await
            .map_err(SqlError::from)?;
        Ok(())
    }

    async fn table_column_types(&self, table: &str) -> SqlResult<TableTypes> {
        if let Some(cached) = self.cached_types(table)? {
            return Ok(cached);
        }

        let rows = sqlx::query(
            "SELECT column_name, data_type FROM information_schema.columns \
             WHERE table_schema = $1 AND table_catalog = current_database() \
               AND table_name = $2",
        )
        .bind(self.schema.as_str())
        .bind(table)
        .fetch_all(&mut *self.connector.acquire().await?)
        .await
        .map_err(SqlError::from)?;

        let mut map = HashMap::new();
        for row in &rows {
            let name: String = row.try_get("column_name").map_err(SqlError::from)?;
            let raw: String = row.try_get("data_type").map_err(SqlError::from)?;
            let column_type = normalize_type(&raw, table, &name);
            // The type name is spliced into the cast, so catalog text is never trusted blindly.
            if column_type.is_empty()
                || !column_type
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b' ')
            {
                return Err(SqlError::Introspection(format!(
                    "column `{name}` of `{table}` has an unsupported type `{column_type}` for sync",
                )));
            }
            map.insert(name, column_type);
        }
        if map.is_empty() {
            return Err(SqlError::Introspection(format!(
                "table `{table}` was not found (or has no columns) for sync",
            )));
        }

        let map = Arc::new(map);
        self.column_types
            .lock()
            .map_err(|_| SqlError::Config("slice store column cache poisoned".to_string()))?
            .insert(table.to_string(), Arc::clone(&map));
        Ok(map)
    }

    fn cached_types(&self, table: &str) -> SqlResult<Option<TableTypes>> {
        Ok(self
            .column_types
            .lock()
            .map_err(|_| SqlError::Config("slice store column cache poisoned".to_string()))?
            .get(table)
            .cloned())
    }
}

/// Narrows a model's key columns to one, refusing a composite key that would match on its leading column alone.
fn single_key_column<'c>(key_columns: &'c [&'c str], table: &str) -> SqlResult<&'c str> {
    match key_columns {
        [only] => Ok(only),
        columns => Err(SqlError::Query(format!(
            "`{table}` is keyed by {} columns ({}); this store addresses a row by one \
             (composite keys need multi-column slice statements)",
            columns.len(),
            columns.join(", "),
        ))),
    }
}

/// Narrows a [`RecordKey`] to one column, refusing a composite one; `what` names whose key it was.
fn single_key<'k>(key: &'k RecordKey, table: &str, what: &str) -> SqlResult<&'k str> {
    key.as_single().ok_or_else(|| {
        SqlError::Query(format!(
            "{what} key for `{table}` names {} columns; this store addresses a row by one \
             (composite keys need ServerSyncable::KEY_COLUMNS)",
            key.len()
        ))
    })
}

fn cast_type<'a>(
    types: &'a HashMap<String, String>,
    table: &str,
    column: &str,
) -> SqlResult<&'a str> {
    types.get(column).map(String::as_str).ok_or_else(|| {
        SqlError::Introspection(format!("table `{table}` has no column `{column}` to sync"))
    })
}

impl SliceStore for SqlSliceStore {
    async fn changed_page(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        SqlSliceStore::changed_page(
            self,
            table,
            data_columns,
            key_columns,
            filters,
            since,
            after,
            limit,
        )
        .await
        .map_err(|e| SyncError::Pull(e.to_string()))
    }

    async fn table_exists(&self, table: &str) -> Result<bool, SyncError> {
        // Bound as parameters, and asked in this store's schema, not `public`.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = $1 AND table_catalog = current_database() \
               AND table_name = $2)",
        )
        .bind(self.schema.as_str())
        .bind(table)
        .fetch_one(
            &mut *self
                .connector
                .acquire()
                .await
                .map_err(|e| SyncError::Pull(e.to_string()))?,
        )
        .await
        .map_err(|e| SyncError::Pull(e.to_string()))?;
        Ok(exists)
    }
}

impl SliceWriteStore for SqlSliceStore {
    async fn upsert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        SqlSliceStore::upsert(self, table, data_columns, key_columns, record)
            .await
            .map_err(|e| SyncError::Push(e.to_string()))
    }
}
