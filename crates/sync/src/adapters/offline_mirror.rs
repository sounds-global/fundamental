//! A [`LocalMirror`] over the offline (SQLite) database.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use fundamental_db::RecordKey;
use fundamental_offline::change::{ChangeOp, TableChange};
use fundamental_offline::db::{OfflineDatabase, Value};
use tracing::instrument;

use crate::engine::{LocalMirror, SyncMeta, SyncRecord};
use crate::{SyncColumnRole, SyncError, Syncable};

/// The high-water mark of the last pull, per mirror table.
const STATE_TABLE: &str = "_fundamental_sync_state";
/// Mirror rows with local changes awaiting a push; `_v2` because the key's encoding changed.
const DIRTY_TABLE: &str = "_fundamental_sync_dirty_v2";

/// A [`LocalMirror`] backed by the offline SQLite mirror table for model `M`.
pub struct OfflineMirror<'a, M: Syncable> {
    db: &'a OfflineDatabase,
    _marker: PhantomData<M>,
}

impl<'a, M: Syncable> OfflineMirror<'a, M> {
    /// Wraps `db`, ensuring the sync sidecar tables exist.
    ///
    /// # Errors
    /// Returns [`SyncError::Storage`] if the sidecar tables cannot be created, or if
    /// `M` declares no key columns at all.
    pub fn new(db: &'a OfflineDatabase) -> Result<Self, SyncError> {
        if M::primary_key().is_empty() {
            return Err(SyncError::Storage(format!(
                "`{}` declares no key columns, so a mirror row cannot be addressed",
                M::MIRROR_TABLE,
            )));
        }
        db.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {STATE_TABLE} \
                 (table_name TEXT PRIMARY KEY, last_pulled INTEGER NOT NULL);\
             CREATE TABLE IF NOT EXISTS {DIRTY_TABLE} \
                 (table_name TEXT NOT NULL, row_id TEXT NOT NULL, \
                  PRIMARY KEY (table_name, row_id));"
        ))
        .map_err(storage)?;
        Ok(Self {
            db,
            _marker: PhantomData,
        })
    }

    /// Records a local edit: upserts the row and marks it dirty for the next push.
    ///
    /// # Errors
    /// Returns [`SyncError::Storage`] if the write fails.
    pub fn record_local_change(&self, record: &SyncRecord) -> Result<(), SyncError> {
        self.upsert_row(record)?;
        let row_id = self.dirty_key(&record.id)?;
        self.db
            .execute(
                &format!(
                    "INSERT OR REPLACE INTO {DIRTY_TABLE} (table_name, row_id) VALUES (?1, ?2)"
                ),
                &[Value::from(M::MIRROR_TABLE), Value::from(row_id)],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn key_columns(&self) -> &'static [&'static str] {
        M::primary_key()
    }

    /// The sidecar's text for a whole key: a netstring, so `("a", "bc")` and `("ab", "c")` differ.
    ///
    /// # Errors
    /// Returns [`SyncError::Storage`] when the key does not address this mirror,
    /// which means the record and the model disagree about the table.
    fn dirty_key(&self, key: &RecordKey) -> Result<String, SyncError> {
        if key.len() != self.key_columns().len() {
            return Err(SyncError::Storage(format!(
                "offline mirror `{}` is addressed by {} column(s); got a {}-column key",
                M::MIRROR_TABLE,
                self.key_columns().len(),
                key.len()
            )));
        }
        Ok(key.to_netstring())
    }

    fn payload_columns(&self) -> impl Iterator<Item = &'static str> {
        let keys = self.key_columns();
        M::sync_columns()
            .iter()
            .filter(move |c| c.role == SyncColumnRole::Data && !keys.contains(&c.name))
            .map(|c| c.name)
    }

    fn upsert_row(&self, record: &SyncRecord) -> Result<(), SyncError> {
        self.dirty_key(&record.id)?;
        let mut columns: Vec<&str> = self.key_columns().to_vec();
        let mut values: Vec<Value> = record
            .id
            .components()
            .map(|value| Value::from(value.to_string()))
            .collect();
        for name in self.payload_columns() {
            columns.push(name);
            values.push(Value::from(
                record.payload.get(name).cloned().unwrap_or_default(),
            ));
        }
        columns.push("updated_at");
        values.push(Value::from(record.meta.updated_at));
        columns.push("version");
        values.push(Value::from(record.meta.version));
        columns.push("_tombstone");
        values.push(Value::from(i64::from(record.meta.tombstone)));

        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
            M::MIRROR_TABLE,
            columns.join(", "),
            placeholders.join(", ")
        );
        self.db.execute(&sql, &values).map_err(storage)?;
        Ok(())
    }

    fn read_row(&self, key: &RecordKey) -> Result<Option<SyncRecord>, SyncError> {
        self.dirty_key(key)?;
        // Every key column, so a composite address selects one row.
        let predicate = self
            .key_columns()
            .iter()
            .enumerate()
            .map(|(at, column)| format!("{column} = ?{}", at + 1))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("SELECT * FROM {} WHERE {predicate}", M::MIRROR_TABLE);
        let binds: Vec<Value> = key
            .components()
            .map(|value| Value::from(value.to_string()))
            .collect();
        let rows = self.db.query(&sql, &binds).map_err(storage)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };

        let mut payload = BTreeMap::new();
        for name in self.payload_columns() {
            let value = row
                .get_named(name)
                .and_then(|v| v.as_text())
                .unwrap_or("")
                .to_string();
            payload.insert(name.to_string(), value);
        }
        let updated_at = row
            .get_named("updated_at")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let version = row
            .get_named("version")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let tombstone = row
            .get_named("_tombstone")
            .and_then(|v| v.as_integer())
            .unwrap_or(0)
            != 0;
        Ok(Some(SyncRecord {
            id: key.clone(),
            meta: SyncMeta {
                updated_at,
                version,
                tombstone,
            },
            payload,
        }))
    }
}

impl<M: Syncable> LocalMirror for OfflineMirror<'_, M> {
    #[instrument(skip(self, _table), fields(table = M::MIRROR_TABLE))]
    async fn get(&self, _table: &str, key: &RecordKey) -> Result<Option<SyncRecord>, SyncError> {
        self.read_row(key)
    }

    #[instrument(skip(self, _table, record), fields(table = M::MIRROR_TABLE, id = %record.id))]
    async fn apply_remote(&self, _table: &str, record: &SyncRecord) -> Result<(), SyncError> {
        self.upsert_row(record)?;
        let row_id = self.dirty_key(&record.id)?;
        self.db
            .execute(
                &format!("DELETE FROM {DIRTY_TABLE} WHERE table_name = ?1 AND row_id = ?2"),
                &[Value::from(M::MIRROR_TABLE), Value::from(row_id.clone())],
            )
            .map_err(storage)?;
        let op = if record.meta.tombstone {
            ChangeOp::Delete
        } else {
            ChangeOp::Update
        };
        self.db
            .notify(TableChange::new(M::MIRROR_TABLE, Value::from(row_id), op));
        Ok(())
    }

    #[instrument(skip(self, _table), fields(table = M::MIRROR_TABLE))]
    async fn dirty(&self, _table: &str) -> Result<Vec<SyncRecord>, SyncError> {
        let ids = self
            .db
            .query(
                &format!("SELECT row_id FROM {DIRTY_TABLE} WHERE table_name = ?1"),
                &[Value::from(M::MIRROR_TABLE)],
            )
            .map_err(storage)?;
        let mut records = Vec::new();
        for row in &ids {
            let Some(text) = row.get_named("row_id").and_then(|v| v.as_text()) else {
                continue;
            };
            // A dirty entry that will not parse was not written by this build; skipped, never guessed at.
            let key = match RecordKey::from_netstring(text) {
                Ok(key) => key,
                Err(why) => {
                    tracing::warn!(
                        table = M::MIRROR_TABLE,
                        error = %why,
                        "unparseable dirty-set entry skipped; it will re-sync from the server"
                    );
                    continue;
                }
            };
            if let Some(record) = self.read_row(&key)? {
                records.push(record);
            }
        }
        Ok(records)
    }

    #[instrument(skip(self, _table), fields(table = M::MIRROR_TABLE))]
    async fn mark_clean(&self, _table: &str, key: &RecordKey) -> Result<(), SyncError> {
        let id = self.dirty_key(key)?;
        self.db
            .execute(
                &format!("DELETE FROM {DIRTY_TABLE} WHERE table_name = ?1 AND row_id = ?2"),
                &[Value::from(M::MIRROR_TABLE), Value::from(id.to_string())],
            )
            .map_err(storage)?;
        Ok(())
    }

    async fn last_pulled(&self, _table: &str) -> Result<i64, SyncError> {
        let rows = self
            .db
            .query(
                &format!("SELECT last_pulled FROM {STATE_TABLE} WHERE table_name = ?1"),
                &[Value::from(M::MIRROR_TABLE)],
            )
            .map_err(storage)?;
        Ok(rows
            .first()
            .and_then(|r| r.get_named("last_pulled"))
            .and_then(|v| v.as_integer())
            .unwrap_or(0))
    }

    async fn set_last_pulled(&self, _table: &str, updated_at: i64) -> Result<(), SyncError> {
        self.db
            .execute(
                &format!(
                    "INSERT OR REPLACE INTO {STATE_TABLE} (table_name, last_pulled) VALUES (?1, ?2)"
                ),
                &[Value::from(M::MIRROR_TABLE), Value::from(updated_at)],
            )
            .map_err(storage)?;
        Ok(())
    }
}

fn storage(error: fundamental_offline::db::OfflineDbError) -> SyncError {
    SyncError::Storage(error.to_string())
}
