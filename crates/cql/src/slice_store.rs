//! Reads and writes a slice's rows in YCQL through a per-owner outbox.
//!
//! YCQL cannot range-scan a non-key `updated_at`, so every change is appended to an outbox
//! keyed for the slice query.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scylla::statement::batch::{Batch, BatchType};
use scylla::value::{CqlValue as DriverCqlValue, Row as DriverRow};
use tracing::{debug, instrument};

use fundamental_db::{Filter, FilterCondition, FilterValue, RecordKey};
use fundamental_sync::wire::SliceCursor;
use fundamental_sync::{
    CaptureTable, SliceStore, SliceWriteStore, SyncError, SyncMeta, SyncRecord,
};

use crate::{Cql, CqlError, CqlExecutor, CqlPool, CqlResult, CqlValue, CqlValues, Ddl};

/// The outbox table every syncable YCQL table shares.
const OUTBOX_TABLE: &str = "sync_outbox_v2";
/// The bucket width: one hour in milliseconds.
const BUCKET_MS: i64 = 3_600_000;
/// Default outbox retention: the longest a device may be offline before it must re-provision.
const DEFAULT_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// One table's column → CQL-type map, cached after the first introspection.
type ColumnTypes = Arc<HashMap<String, String>>;

/// Serves slices over YCQL by appending every change to a shared outbox table
/// keyed for the slice query.
pub struct CqlSliceStore {
    /// Every statement runs through here, so the sync path is prepared, paged and measured.
    cql: CqlExecutor,
    keyspace: String,
    owners: HashMap<String, String>,
    partitions: HashMap<String, Vec<String>>,
    column_types: Arc<Mutex<HashMap<String, ColumnTypes>>>,
    retention_secs: i32,
}

impl CqlSliceStore {
    /// Builds a store over `pool`'s `keyspace`, serving the given `tables` (each
    /// supplies the owner column that scopes its rows).
    #[must_use]
    pub fn new(
        pool: &CqlPool,
        keyspace: impl Into<String>,
        tables: impl IntoIterator<Item = CaptureTable>,
    ) -> Self {
        let (owners, partitions) = tables
            .into_iter()
            .map(|t| {
                (
                    (t.table().to_string(), t.owner_column().to_string()),
                    (t.table().to_string(), t.partition_columns().to_vec()),
                )
            })
            .collect();
        let keyspace = keyspace.into();
        Self {
            cql: pool.cql_for(keyspace.clone()),
            keyspace,
            owners,
            partitions,
            column_types: Arc::new(Mutex::new(HashMap::new())),
            retention_secs: clamp_secs(DEFAULT_RETENTION),
        }
    }

    /// Overrides the outbox retention (how long a changed row stays pullable).
    #[must_use]
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention_secs = clamp_secs(retention);
        self
    }

    /// Creates the shared `sync_outbox` table if it does not exist.
    ///
    /// # Errors
    /// Returns a [`CqlError`] if the DDL fails.
    pub async fn create_outbox_table(&self) -> CqlResult<()> {
        let cql = format!(
            "CREATE TABLE IF NOT EXISTS {}.{OUTBOX_TABLE} (\
                 owner text, table_name text, bucket bigint, \
                 updated_at bigint, id text, version bigint, \
                 tombstone boolean, payload text, \
                 PRIMARY KEY ((owner, table_name, bucket), updated_at, id))",
            self.keyspace
        );
        self.cql.execute_ddl(&Ddl::interpolated(cql)).await
    }

    /// Reads up to `limit` changed rows of `table` for the owner named by `filters`,
    /// in `(updated_at, id)` order, after the keyset point `after` (or after the
    /// high-water mark `since`).
    ///
    /// # Errors
    /// - [`CqlError::InvalidQuery`] if `table` is not registered, the filter is
    ///   not an equality on the owner column, or (with no cursor) `since` predates
    ///   the retention window — a full resync past the TTL horizon is unsupported.
    /// - [`CqlError::Query`] / [`CqlError::RowConversion`] on a read failure.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub async fn changed_page(
        &self,
        table: &str,
        key_columns: &[&str],
        filters: &[Filter],
        since: i64,
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> CqlResult<Vec<SyncRecord>> {
        let now = current_millis();
        let horizon = now - i64::from(self.retention_secs) * 1000;
        // Changes older than the retention window aged out, so the device must re-provision.
        if after.is_none() && since < horizon {
            return Err(CqlError::invalid_query(
                "high-water mark predates the outbox retention window; a full resync is \
                 unsupported on CqlDb — re-provision the device",
            ));
        }
        let owner_column = self.owner_column(table)?;
        let partition_columns = self.partition_columns(table)?;
        let _ = owner_column;
        let partition = self.partition_from_filters(filters, table)?;
        let owner = netstring(&partition.iter().map(String::as_str).collect::<Vec<_>>());

        let mark = after.map_or(since, |c| c.updated_at);
        // Clamped to the retention horizon: older buckets hold only expired rows.
        let lower_bucket = (mark / BUCKET_MS).max(horizon / BUCKET_MS);
        let upper_bucket = now / BUCKET_MS + 1;

        let mut records = Vec::new();
        let mut bucket = lower_bucket;
        while bucket <= upper_bucket && records.len() < limit as usize {
            let remaining = (limit as usize - records.len()).min(i32::MAX as usize) as i32;
            let page = self
                .read_bucket(
                    table,
                    key_columns,
                    partition_columns,
                    &owner,
                    bucket,
                    since,
                    after,
                    bucket == lower_bucket,
                    remaining,
                )
                .await?;
            records.extend(page);
            bucket += 1;
        }
        Ok(records)
    }

    /// Reads one page of a cold-start snapshot from the base table: the owner's current rows,
    /// ordered by primary key after `after`.
    ///
    /// Each row is stamped with one snapshot watermark `T`, carried across pages in the cursor, so
    /// the next pull is an ordinary outbox delta from `T`. LWW may let a snapshot win over an old,
    /// unpushed local row on a device idle past the retention horizon.
    ///
    /// # Errors
    /// Returns a [`CqlError`] if `table` is unregistered, the owner equality filter is
    /// absent, or the read fails — including a non-text data column, which a snapshot reads as
    /// text (the delta path serves every type via the outbox's JSON payload).
    #[expect(
        clippy::too_many_lines,
        clippy::too_many_arguments,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    async fn snapshot_page(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        filters: &[Filter],
        after: Option<&SliceCursor>,
        limit: u32,
    ) -> CqlResult<Vec<SyncRecord>> {
        let partition_columns = self.partition_columns(table)?;
        let partition = self.partition_from_filters(filters, table)?;
        // The watermark: taken now on the first page, carried in the cursor on later pages so
        // every row of the snapshot — across all pages — shares one `updated_at`.
        let watermark = after.map_or_else(current_millis, |cursor| cursor.updated_at);

        // Every key column, then the data columns; `data_columns()` excludes the key.
        let mut columns: Vec<&str> = key_columns.to_vec();
        columns.extend(data_columns.iter().copied());

        let predicate = partition_columns
            .iter()
            .map(|column| format!("{column} = ?"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut cql = format!(
            "SELECT {} FROM {}.{table} WHERE {predicate}",
            columns.join(", "),
            self.keyspace
        );
        let mut values: Vec<CqlValue> = partition.iter().cloned().map(CqlValue::text).collect();
        if let Some(cursor) = after {
            // Resume after the clustering component: the partition is already pinned
            // by the owner equality above, so the in-partition order is what pages.
            let resume = self
                .row_address(&cursor.key, key_columns, partition_columns, table)?
                .outbox_id
                .to_string();
            let clustering = key_columns.last().copied().unwrap_or_default();
            cql.push_str(&format!(" AND {clustering} > ?"));
            values.push(CqlValue::text(resume));
        }
        cql.push_str(" LIMIT ?");
        values.push(CqlValue::Int(i32::try_from(limit).unwrap_or(i32::MAX)));

        let rows = self
            .cql
            .fetch_paged::<DriverRow>(
                &Cql::interpolated(cql).values_of(values).idempotent(),
                limit as usize,
            )
            .await?;

        let mut records = Vec::new();
        for cells in rows {
            let mut cells = cells.columns.into_iter();
            let mut key_values: Vec<String> = Vec::with_capacity(key_columns.len());
            for column in key_columns {
                let value = cells
                    .next()
                    .flatten()
                    .and_then(|value| cql_text(&value))
                    .ok_or_else(|| {
                        CqlError::row_conversion(format!(
                            "snapshot row of `{table}` is missing its key column `{column}`"
                        ))
                    })?;
                key_values.push(value);
            }
            let mut key_values = key_values.into_iter();
            let Some(first) = key_values.next() else {
                return Err(CqlError::invalid_query(format!(
                    "`{table}` declares no key columns"
                )));
            };
            let mut payload = BTreeMap::new();
            for column in columns.iter().skip(key_columns.len()) {
                if let Some(text) = cells.next().flatten().as_ref().and_then(cql_text) {
                    payload.insert((*column).to_string(), text);
                }
            }
            records.push(SyncRecord {
                id: RecordKey::composite(first, key_values),
                meta: SyncMeta {
                    updated_at: watermark,
                    version: 1,
                    tombstone: false,
                },
                payload,
            });
        }
        Ok(records)
    }

    /// Inserts or updates `record` in `table` and appends it to the outbox in one
    /// logged batch; a tombstone deletes the base row and records the delete.
    ///
    /// # Errors
    /// Returns a [`CqlError`] if `table` is not registered, a value cannot be
    /// coerced to its column type, or the batch fails.
    pub async fn upsert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> CqlResult<()> {
        // The partition and row address come from the key: a key column is not in the payload.
        let partition_columns = self.partition_columns(table)?;
        let address = self.row_address(&record.id, key_columns, partition_columns, table)?;
        let owner = address.owner.to_string();
        let id = address.outbox_id.to_string();
        let now = current_millis();
        let bucket = now / BUCKET_MS;
        let types = self.column_types(table).await?;

        let payload = serde_json::to_string(&record.payload)
            .map_err(|e| CqlError::row_conversion(format!("encoding sync payload: {e}")))?;
        let outbox_cql = format!(
            "INSERT INTO {}.{OUTBOX_TABLE} \
                 (owner, table_name, bucket, updated_at, id, version, tombstone, payload) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) USING TTL {}",
            self.keyspace, self.retention_secs
        );
        let outbox_values = CqlValues(vec![
            CqlValue::text(owner.clone()),
            CqlValue::text(table.to_string()),
            CqlValue::BigInt(bucket),
            CqlValue::BigInt(now),
            CqlValue::text(id.clone()),
            CqlValue::BigInt(record.meta.version),
            CqlValue::Bool(record.meta.tombstone),
            CqlValue::text(payload),
        ]);

        let (base_cql, base_values) = if record.meta.tombstone {
            // Every key column, so the delete addresses exactly one row.
            let predicate = key_columns
                .iter()
                .map(|column| format!("{column} = ?"))
                .collect::<Vec<_>>()
                .join(" AND ");
            let cql = format!("DELETE FROM {}.{table} WHERE {predicate}", self.keyspace);
            let mut values = Vec::with_capacity(key_columns.len());
            for (column, value) in key_columns.iter().zip(record.id.components()) {
                values.push(to_cql(&types, column, value)?);
            }
            (cql, CqlValues(values))
        } else {
            self.base_insert(table, data_columns, key_columns, record, &types)?
        };

        let mut batch = Batch::new(BatchType::Logged);
        batch.append_statement(base_cql.as_str());
        batch.append_statement(outbox_cql.as_str());
        self.cql
            .session()
            .batch(&batch, (base_values, outbox_values))
            .await
            .map_err(|e| CqlError::Query(Box::new(e)))?;
        debug!(
            table,
            tombstone = record.meta.tombstone,
            "slice change written to base + outbox"
        );
        Ok(())
    }

    /// Splits a record's key at the partition boundary into the two cells the
    /// outbox is keyed by.
    ///
    /// Each cell is a netstring, arity 1 included, so it decodes unambiguously; the split is the
    /// partition boundary, because the outbox's partition must be the base table's.
    fn row_address(
        &self,
        key: &RecordKey,
        key_columns: &[&str],
        partition_columns: &[String],
        table: &str,
    ) -> CqlResult<RowAddress> {
        if key.len() != key_columns.len() {
            return Err(CqlError::invalid_query(format!(
                "`{table}` is keyed by {} columns but the record's key has {}",
                key_columns.len(),
                key.len()
            )));
        }
        let at = partition_columns.len();
        if at == 0 || at > key_columns.len() {
            return Err(CqlError::invalid_query(format!(
                "`{table}` declares {at} partition column(s) for a {}-column key",
                key_columns.len()
            )));
        }
        let mut components = key.components();
        let partition: Vec<&str> = components.by_ref().take(at).collect();
        let clustering: Vec<&str> = components.collect();
        Ok(RowAddress {
            owner: netstring(&partition),
            outbox_id: netstring(&clustering),
        })
    }

    /// Rebuilds a record's whole key from the outbox's two cells.
    ///
    /// # Errors
    ///
    /// [`CqlError::RowConversion`] when a cell is not a netstring this build wrote.
    fn record_key(owner: &str, outbox_id: &str, table: &str) -> CqlResult<RecordKey> {
        let mut components = parse_netstring(owner, table)?;
        components.extend(parse_netstring(outbox_id, table)?);
        let mut components = components.into_iter();
        let Some(first) = components.next() else {
            return Err(CqlError::row_conversion(format!(
                "an outbox row of `{table}` encodes no key components"
            )));
        };
        Ok(RecordKey::composite(first, components))
    }

    /// Builds the typed base-table `INSERT` for a live record (primary key plus the
    /// data columns, each coerced from text to its column type).
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    fn base_insert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
        types: &HashMap<String, String>,
    ) -> CqlResult<(String, CqlValues)> {
        // Key columns from the key, data columns from the payload; the two sets are disjoint.
        let mut columns: Vec<String> = Vec::with_capacity(key_columns.len() + data_columns.len());
        let mut values = Vec::with_capacity(columns.capacity());
        for (column, value) in key_columns.iter().zip(record.id.components()) {
            columns.push((*column).to_string());
            values.push(to_cql(types, column, value)?);
        }
        if columns.len() != key_columns.len() {
            return Err(CqlError::invalid_query(format!(
                "`{table}` is keyed by {} columns but the pushed record's key has {}",
                key_columns.len(),
                record.id.len()
            )));
        }
        for column in data_columns {
            columns.push((*column).to_string());
            match record.payload.get(*column) {
                Some(text) => values.push(to_cql(types, column, text)?),
                None => values.push(CqlValue::Null),
            }
        }
        let placeholders = vec!["?"; columns.len()].join(", ");
        let cql = format!(
            "INSERT INTO {}.{table} ({}) VALUES ({placeholders})",
            self.keyspace,
            columns.join(", ")
        );
        Ok((cql, CqlValues(values)))
    }

    /// Reads one outbox bucket partition.
    ///
    /// YCQL has no tuple comparison `(updated_at, id) > (?, ?)`, so a resumed page reads
    /// `updated_at = ? AND id > ?`, then `updated_at > ?`.
    #[allow(clippy::too_many_arguments)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    async fn read_bucket(
        &self,
        table: &str,
        key_columns: &[&str],
        partition_columns: &[String],
        owner: &str,
        bucket: i64,
        since: i64,
        after: Option<&SliceCursor>,
        is_lower: bool,
        limit: i32,
    ) -> CqlResult<Vec<SyncRecord>> {
        // Only the bucket holding the mark needs a lower bound; later buckets are
        // entirely newer than the mark.
        let ranges: Vec<(&str, Vec<CqlValue>)> = match (is_lower, after) {
            (true, Some(cursor)) => {
                let resume = self
                    .row_address(&cursor.key, key_columns, partition_columns, table)?
                    .outbox_id
                    .to_string();
                vec![
                    (
                        " AND updated_at = ? AND id > ?",
                        vec![CqlValue::BigInt(cursor.updated_at), CqlValue::text(resume)],
                    ),
                    (
                        " AND updated_at > ?",
                        vec![CqlValue::BigInt(cursor.updated_at)],
                    ),
                ]
            }
            (true, None) => vec![(" AND updated_at > ?", vec![CqlValue::BigInt(since)])],
            (false, _) => vec![("", Vec::new())],
        };

        let mut records = Vec::new();
        for (predicate, bound) in ranges {
            let remaining = limit - records.len() as i32;
            if remaining <= 0 {
                break;
            }
            // Each range asks only for what the page still has room for, so the two
            // together never read more than one page's worth.
            records.extend(
                self.read_range(table, owner, bucket, predicate, bound, remaining)
                    .await?,
            );
        }
        Ok(records)
    }

    /// One `SELECT` over an outbox partition, `predicate` narrowing the clustering range
    /// and `bound` supplying its parameters.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    async fn read_range(
        &self,
        table: &str,
        owner: &str,
        bucket: i64,
        predicate: &str,
        bound: Vec<CqlValue>,
        limit: i32,
    ) -> CqlResult<Vec<SyncRecord>> {
        let cql = format!(
            "SELECT updated_at, id, version, tombstone, payload FROM {}.{OUTBOX_TABLE} \
             WHERE owner = ? AND table_name = ? AND bucket = ?{predicate} LIMIT ?",
            self.keyspace
        );
        let mut values = vec![
            CqlValue::text(owner.to_string()),
            CqlValue::text(table.to_string()),
            CqlValue::BigInt(bucket),
        ];
        values.extend(bound);
        values.push(CqlValue::Int(limit));

        let rows = self
            .cql
            .fetch_paged::<(i64, String, i64, bool, String)>(
                &Cql::interpolated(cql).values_of(values).idempotent(),
                limit as usize,
            )
            .await?;

        let mut records = Vec::with_capacity(rows.len());
        for (updated_at, id, version, tombstone, payload) in rows {
            let payload: BTreeMap<String, String> = serde_json::from_str(&payload)
                .map_err(|e| CqlError::row_conversion(format!("decoding sync payload: {e}")))?;
            records.push(SyncRecord {
                id: Self::record_key(owner, &id, table)?,
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

    /// The outbox partition cell a slice's resolved predicate selects.
    ///
    /// Every partition column must be pinned by an equality: an unpinned one is a cross-partition
    /// read, and for a zone-led key the partitions are jurisdictions.
    fn partition_from_filters(&self, filters: &[Filter], table: &str) -> CqlResult<Vec<String>> {
        let columns = self.partition_columns(table)?;
        let mut values: Vec<String> = Vec::with_capacity(columns.len());
        for column in columns {
            let value = owner_from_filters(filters, column).ok_or_else(|| {
                CqlError::invalid_query(format!(
                    "`{table}` is partitioned by ({}); the slice must pin every one with an \
                     equality filter, and `{column}` is not pinned",
                    columns.join(", ")
                ))
            })?;
            values.push(value);
        }
        Ok(values)
    }

    /// The partition columns registered for `table`.
    fn partition_columns(&self, table: &str) -> CqlResult<&[String]> {
        self.partitions
            .get(table)
            .map(Vec::as_slice)
            .ok_or_else(|| {
                CqlError::invalid_query(format!(
                    "table `{table}` is not registered with the outbox"
                ))
            })
    }

    /// The owner column registered for `table`.
    fn owner_column(&self, table: &str) -> CqlResult<&str> {
        self.owners.get(table).map(String::as_str).ok_or_else(|| {
            CqlError::invalid_query(format!("table `{table}` is not registered with the outbox"))
        })
    }

    /// Returns `table`'s column → CQL-type map, introspecting `system_schema` on
    /// first use and caching it.
    async fn column_types(&self, table: &str) -> CqlResult<ColumnTypes> {
        if let Some(cached) = self
            .column_types
            .lock()
            .map_err(|_| CqlError::invalid_query("outbox column cache poisoned"))?
            .get(table)
            .cloned()
        {
            return Ok(cached);
        }

        let statement = Cql::new(
            "SELECT column_name, type FROM system_schema.columns \
             WHERE keyspace_name = ? AND table_name = ?",
        )
        .bind(CqlValue::text(self.keyspace.as_str()))
        .bind(CqlValue::text(table))
        .idempotent();
        let map: HashMap<String, String> = self
            .cql
            .fetch_all::<(String, String)>(&statement)
            .await?
            .into_iter()
            .collect();
        if map.is_empty() {
            return Err(CqlError::invalid_query(format!(
                "table `{table}` was not found for sync"
            )));
        }
        let map = Arc::new(map);
        self.column_types
            .lock()
            .map_err(|_| CqlError::invalid_query("outbox column cache poisoned"))?
            .insert(table.to_string(), Arc::clone(&map));
        Ok(map)
    }
}

impl SliceStore for CqlSliceStore {
    #[instrument(
        name = "cql.slice.changed_page",
        skip_all,
        fields(%table, limit, has_cursor = after.is_some())
    )]
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
        // A mark older than the retention window bootstraps from the base table; `since` is
        // constant across a pull's pages, so the cold start stays in snapshot mode.
        let horizon = current_millis() - i64::from(self.retention_secs) * 1000;
        if since < horizon {
            return self
                .snapshot_page(table, data_columns, key_columns, filters, after, limit)
                .await
                .map_err(|e| SyncError::Pull(e.to_string()));
        }
        let _ = data_columns; // the outbox carries a self-contained delta payload
        CqlSliceStore::changed_page(self, table, key_columns, filters, since, after, limit)
            .await
            .map_err(|e| SyncError::Pull(e.to_string()))
    }

    async fn table_exists(&self, table: &str) -> Result<bool, SyncError> {
        // `table` is bound as a parameter (never spliced); `system_schema.tables`
        // is authoritative for this keyspace.
        let statement = Cql::new(
            "SELECT table_name FROM system_schema.tables \
             WHERE keyspace_name = ? AND table_name = ?",
        )
        .bind(CqlValue::text(self.keyspace.as_str()))
        .bind(CqlValue::text(table))
        .idempotent();
        Ok(!self
            .cql
            .fetch_paged::<(String,)>(&statement, 1)
            .await
            .map_err(|e| SyncError::Pull(e.to_string()))?
            .is_empty())
    }
}

impl SliceWriteStore for CqlSliceStore {
    #[instrument(name = "cql.slice.upsert", skip_all, fields(%table))]
    async fn upsert(
        &self,
        table: &str,
        data_columns: &[&str],
        key_columns: &[&str],
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        CqlSliceStore::upsert(self, table, data_columns, key_columns, record)
            .await
            .map_err(|e| SyncError::Push(e.to_string()))
    }
}

/// Milliseconds since the Unix epoch, or `0` if the clock is before the epoch.
fn current_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Renders a base-table column value as the sync payload's text, the inverse of [`to_cql`];
/// `None` for a null or a type a snapshot does not render.
fn cql_text(value: &DriverCqlValue) -> Option<String> {
    if let Some(text) = value.as_text() {
        return Some(text.clone());
    }
    if let Some(v) = value.as_bigint() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_int() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_boolean() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_double() {
        return Some(v.to_string());
    }
    if let Some(v) = value.as_uuid() {
        return Some(v.to_string());
    }
    None
}

/// Clamps a retention duration to the `i32` second range a CQL `TTL` accepts.
fn clamp_secs(retention: Duration) -> i32 {
    retention.as_secs().min(i32::MAX as u64) as i32
}

/// Where a row sits in the outbox: which partition, and which clustering id.
struct RowAddress {
    /// The outbox partition cell — a netstring of the key's partition components.
    owner: String,
    /// The outbox clustering cell — a netstring of the key's clustering components.
    outbox_id: String,
}

/// Renders components into one self-delimiting text cell.
///
/// `{len}:{bytes},` per component.
fn netstring(components: &[&str]) -> String {
    let mut out = String::new();
    for component in components {
        out.push_str(&component.len().to_string());
        out.push(':');
        out.push_str(component);
        out.push(',');
    }
    out
}

/// Parses [`netstring`].
///
/// # Errors
/// Returns [`CqlError::RowConversion`] naming `table` when the text is not a
/// netstring sequence — a stored cell this build did not write.
fn parse_netstring(text: &str, table: &str) -> CqlResult<Vec<String>> {
    // The cell holds the components of a key a device pushed, so it is named by its length
    // and never quoted.
    let malformed = |what: &str| {
        CqlError::row_conversion(format!(
            "an outbox cell of `{table}` is not a netstring ({what}); {} bytes",
            text.len()
        ))
    };
    let bytes = text.as_bytes();
    let mut components = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let colon = text[at..].find(':').ok_or_else(|| malformed("no length"))? + at;
        let len: usize = text[at..colon]
            .parse()
            .map_err(|_| malformed("length is not a number"))?;
        let start = colon + 1;
        let end = start + len;
        if end >= bytes.len() || bytes[end] != b',' {
            return Err(malformed("component is not terminated"));
        }
        components.push(
            text.get(start..end)
                .ok_or_else(|| malformed("component is not valid UTF-8"))?
                .to_owned(),
        );
        at = end + 1;
    }
    Ok(components)
}

/// Finds the value an equality pins `column` to, anywhere a conjunction reaches.
///
/// Descends into `And`, where multi-column pins live, but not into `Or` or `Not`, which pin
/// nothing.
fn owner_from_filters(filters: &[Filter], column_name: &str) -> Option<String> {
    filters.iter().find_map(|filter| match &filter.condition {
        FilterCondition::Eq { column, value } if column == column_name => filter_value_text(value),
        FilterCondition::And(inner) => owner_from_filters(inner, column_name),
        _ => None,
    })
}

/// Renders a filter value as the owner text (owners are text/uuid scoping values).
fn filter_value_text(value: &FilterValue) -> Option<String> {
    match value {
        FilterValue::String(s) | FilterValue::Uuid(s) => Some(s.clone()),
        FilterValue::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

/// Coerces a text payload value to the [`CqlValue`] matching `column`'s CQL type,
/// so the driver binds it to a typed column.
fn to_cql(types: &HashMap<String, String>, column: &str, text: &str) -> CqlResult<CqlValue> {
    let cql_type = types.get(column).map(String::as_str).ok_or_else(|| {
        CqlError::invalid_query(format!("table has no column `{column}` to sync"))
    })?;
    // The value is the pushing client's, so the error names the column and the type it
    // expected and never quotes what arrived.
    let parse_err = |kind: &str| {
        CqlError::row_conversion(format!(
            "a pushed value is not a valid {kind} for column `{column}`"
        ))
    };
    match cql_type {
        "text" | "ascii" | "varchar" => Ok(CqlValue::Text(text.to_string())),
        "bigint" => text
            .parse()
            .map(CqlValue::BigInt)
            .map_err(|_| parse_err("bigint")),
        "int" => text
            .parse()
            .map(CqlValue::Int)
            .map_err(|_| parse_err("int")),
        "boolean" => text
            .parse()
            .map(CqlValue::Bool)
            .map_err(|_| parse_err("boolean")),
        "double" | "float" => text
            .parse()
            .map(CqlValue::Double)
            .map_err(|_| parse_err("double")),
        "uuid" | "timeuuid" => text
            .parse()
            .map(CqlValue::Uuid)
            .map_err(|_| parse_err("uuid")),
        "timestamp" => text
            .parse()
            .map(CqlValue::Timestamp)
            .map_err(|_| parse_err("timestamp")),
        other => Err(CqlError::invalid_query(format!(
            "column `{column}` has an unsupported type `{other}` for sync"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> HashMap<String, String> {
        [
            ("id", "uuid"),
            ("owner_id", "text"),
            ("count", "bigint"),
            ("live", "boolean"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn owner_is_read_from_an_eq_filter_on_the_owner_column() {
        let filters = vec![Filter::eq("owner_id", "alice")];
        assert_eq!(
            owner_from_filters(&filters, "owner_id"),
            Some("alice".to_string())
        );
    }

    #[test]
    fn a_non_owner_or_non_eq_filter_yields_no_owner() {
        assert_eq!(
            owner_from_filters(&[Filter::eq("other", "x")], "owner_id"),
            None
        );
        assert_eq!(
            owner_from_filters(&[Filter::gt("owner_id", 1)], "owner_id"),
            None
        );
    }

    #[test]
    fn a_column_pinned_inside_an_and_is_found() {
        // The shape a slice with more than one partition column writes.
        let filters = vec![Filter::and(vec![
            Filter::eq("zone", "eu"),
            Filter::eq("owner_id", "alice"),
        ])];
        assert_eq!(
            owner_from_filters(&filters, "zone"),
            Some("eu".to_string()),
            "the leading partition column is pinned by this predicate"
        );
        assert_eq!(
            owner_from_filters(&filters, "owner_id"),
            Some("alice".to_string()),
            "and so is the second — one `And` has to answer for every column"
        );
    }

    #[test]
    fn a_nested_and_is_still_reached() {
        let filters = vec![Filter::and(vec![
            Filter::eq("zone", "eu"),
            Filter::and(vec![Filter::eq("owner_id", "alice")]),
        ])];
        assert_eq!(
            owner_from_filters(&filters, "owner_id"),
            Some("alice".to_string())
        );
    }

    #[test]
    fn an_equality_under_an_or_pins_nothing() {
        // The other branch may match any partition.
        let filters = vec![Filter::or(vec![
            Filter::eq("owner_id", "alice"),
            Filter::eq("owner_id", "bob"),
        ])];
        assert_eq!(owner_from_filters(&filters, "owner_id"), None);
    }

    #[test]
    fn an_equality_under_a_not_pins_nothing() {
        // A negated equality pins the complement — every partition but this one.
        let filters = vec![Filter::not(Filter::eq("owner_id", "alice"))];
        assert_eq!(owner_from_filters(&filters, "owner_id"), None);
    }

    #[test]
    fn an_and_that_pins_no_matching_column_yields_no_owner() {
        let filters = vec![Filter::and(vec![
            Filter::eq("other", "x"),
            Filter::gt("owner_id", 1),
        ])];
        assert_eq!(owner_from_filters(&filters, "owner_id"), None);
    }

    #[test]
    fn to_cql_coerces_text_to_the_column_type() {
        let types = types();
        assert!(matches!(
            to_cql(&types, "owner_id", "alice"),
            Ok(CqlValue::Text(_))
        ));
        assert!(matches!(
            to_cql(&types, "count", "42"),
            Ok(CqlValue::BigInt(42))
        ));
        assert!(matches!(
            to_cql(&types, "live", "true"),
            Ok(CqlValue::Bool(true))
        ));
        assert!(matches!(
            to_cql(&types, "id", "00000000-0000-0000-0000-000000000001"),
            Ok(CqlValue::Uuid(_))
        ));
    }

    #[test]
    fn a_malformed_outbox_cell_is_never_quoted() {
        let refused = parse_netstring("jan@example.nl", "notes").expect_err("not a netstring");
        let shown = format!("{refused} {refused:?}");
        assert!(shown.contains("notes"), "names the table: {shown}");
        assert!(
            !shown.contains("jan@example.nl"),
            "quotes the cell: {shown}"
        );
    }

    #[test]
    fn to_cql_rejects_a_malformed_value() {
        let refused = to_cql(&types(), "count", "jan@example.nl").expect_err("not a bigint");
        let shown = format!("{refused} {refused:?}");
        assert!(shown.contains("count"), "names the column: {shown}");
        assert!(
            !shown.contains("jan@example.nl"),
            "quotes the value: {shown}"
        );
    }

    #[test]
    fn to_cql_rejects_an_unknown_column() {
        assert!(to_cql(&types(), "missing", "x").is_err());
    }

    #[test]
    fn retention_clamps_into_the_ttl_range() {
        assert_eq!(clamp_secs(Duration::from_secs(3600)), 3600);
        assert_eq!(clamp_secs(Duration::from_secs(u64::MAX)), i32::MAX);
    }
}
