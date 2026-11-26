//! Model-level storage over the offline database.

mod key_of;
mod record;
mod sync;

pub use record::OfflineRecord;
pub use sync::{PendingChange, SyncOperation, SyncQueue};

use std::collections::HashMap;

use fundamental_core::i18n::{Checked, TranslationCheck, TranslationRow};
use fundamental_core::model::OfflineModel;
use tracing::{debug, info, instrument, warn};

use crate::change::{ChangeOp, TableChange};
use crate::db::{OfflineDatabase, OfflineDbError, Value};
use key_of::KeyOf;

const SCHEMA_TABLE: &str = "_fundamental_schema";

/// What [`OfflineStore::ensure_schema`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaAction {
    /// Table did not exist and was created.
    Created,
    /// Table exists with the current schema version.
    Unchanged,
    /// Schema version changed; the table was dropped and recreated.
    Recreated,
}

/// Model-level store over an [`OfflineDatabase`].
#[derive(Debug)]
pub struct OfflineStore<'a> {
    db: &'a OfflineDatabase,
}

impl<'a> OfflineStore<'a> {
    /// Creates a store over the given database.
    pub fn new(db: &'a OfflineDatabase) -> Self {
        Self { db }
    }

    /// Returns the underlying database.
    pub fn db(&self) -> &OfflineDatabase {
        self.db
    }

    /// Emits a change for `M`'s table; no watcher is fine.
    fn emit<M: OfflineModel>(&self, id: Value, op: ChangeOp) {
        self.db.notify(TableChange::new(M::SQLITE_TABLE, id, op));
    }

    /// Ensures the table for `M` exists with the current schema version.
    ///
    /// On a version mismatch a model without `PRESERVE_DATA` is dropped and recreated;
    /// one with it returns [`OfflineDbError::SchemaPreserved`].
    #[instrument(
        skip(self),
        fields(table = M::SQLITE_TABLE, schema_version = M::SCHEMA_VERSION),
        err(Debug)
    )]
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn ensure_schema<M: OfflineModel>(&self) -> Result<SchemaAction, OfflineDbError> {
        self.ensure_schema_table()?;

        let current = M::SCHEMA_VERSION;
        let stored = self
            .db
            .query_optional(
                &format!("SELECT schema_version FROM {SCHEMA_TABLE} WHERE table_name = ?1;"),
                &[Value::from(M::SQLITE_TABLE)],
            )?
            .and_then(|row| row.get(0).and_then(Value::as_integer))
            .map(|v| v as u64);

        match stored {
            None => {
                let tx = self.db.transaction()?;
                tx.execute_batch(&format!("{};", M::create_table_sql()))?;
                if let Some(tsql) = M::translation_table_sql() {
                    tx.execute_batch(&format!("{tsql};"))?;
                }
                tx.execute(
                    &format!(
                        "INSERT INTO {SCHEMA_TABLE} (table_name, schema_version) VALUES (?1, ?2);"
                    ),
                    &[Value::from(M::SQLITE_TABLE), Value::Integer(current as i64)],
                )?;
                tx.commit()?;
                info!("created offline table");
                Ok(SchemaAction::Created)
            }
            Some(stored) if stored == current => {
                debug!("offline table schema unchanged");
                Ok(SchemaAction::Unchanged)
            }
            Some(stored) => {
                if M::PRESERVE_DATA {
                    return Err(OfflineDbError::SchemaPreserved {
                        table: M::SQLITE_TABLE.to_string(),
                        stored,
                        current,
                    });
                }

                // The offline copy is a cache; warn so the data loss is never silent.
                warn!(
                    stored,
                    current,
                    "offline schema version changed; dropping and recreating table (cached data discarded)"
                );
                let tx = self.db.transaction()?;
                let drop_companion = M::translation_table()
                    .map(|t| format!("DROP TABLE IF EXISTS {t};\n"))
                    .unwrap_or_default();
                let create_companion = M::translation_table_sql()
                    .map(|s| format!("\n{s};"))
                    .unwrap_or_default();
                tx.execute_batch(&format!(
                    "{}DROP TABLE IF EXISTS {};\n{};{}",
                    drop_companion,
                    M::SQLITE_TABLE,
                    M::create_table_sql(),
                    create_companion,
                ))?;
                tx.execute(
                    &format!(
                        "UPDATE {SCHEMA_TABLE} SET schema_version = ?2 WHERE table_name = ?1;"
                    ),
                    &[Value::from(M::SQLITE_TABLE), Value::Integer(current as i64)],
                )?;
                tx.commit()?;
                Ok(SchemaAction::Recreated)
            }
        }
    }

    /// Inserts a record. Fails if the primary key already exists.
    #[instrument(skip(self, model), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn insert<M: OfflineRecord>(&self, model: &M) -> Result<(), OfflineDbError> {
        let columns = M::column_names();
        let placeholders = placeholders(columns.len());
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({});",
            M::SQLITE_TABLE,
            columns.join(", "),
            placeholders
        );
        match M::translation_table() {
            None => {
                self.db.execute(&sql, &model.to_values())?;
            }
            Some(ttable) => {
                let tx = self.db.transaction()?;
                tx.execute(&sql, &model.to_values())?;
                replace_translations::<M>(
                    &tx,
                    ttable,
                    &primary_key_value(model),
                    &model.translation_rows(),
                )?;
                tx.commit()?;
            }
        }
        self.emit::<M>(primary_key_value(model), ChangeOp::Insert);
        Ok(())
    }

    /// Inserts or updates a record by primary key.
    #[instrument(skip(self, model), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn upsert<M: OfflineRecord>(&self, model: &M) -> Result<(), OfflineDbError> {
        let columns = M::column_names();
        let pk = M::primary_key_column();
        let placeholders = placeholders(columns.len());

        let updates: Vec<String> = columns
            .iter()
            .filter(|c| **c != pk)
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();

        let conflict_clause = if updates.is_empty() {
            "DO NOTHING".to_string()
        } else {
            format!("DO UPDATE SET {}", updates.join(", "))
        };

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT({}) {};",
            M::SQLITE_TABLE,
            columns.join(", "),
            placeholders,
            pk,
            conflict_clause
        );
        match M::translation_table() {
            None => {
                self.db.execute(&sql, &model.to_values())?;
            }
            Some(ttable) => {
                // Re-write the companion rows so a removed locale is dropped too.
                let tx = self.db.transaction()?;
                tx.execute(&sql, &model.to_values())?;
                replace_translations::<M>(
                    &tx,
                    ttable,
                    &primary_key_value(model),
                    &model.translation_rows(),
                )?;
                tx.commit()?;
            }
        }
        self.emit::<M>(primary_key_value(model), ChangeOp::Update);
        Ok(())
    }

    /// Fetches a record by primary key.
    ///
    /// Excludes soft-deleted rows; see [`find_with_deleted`](Self::find_with_deleted).
    #[instrument(level = "debug", skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn get<M: OfflineRecord>(&self, id: &Value) -> Result<Option<M>, OfflineDbError> {
        self.get_inner::<M>(id, M::SOFT_DELETE)
    }

    fn get_inner<M: OfflineRecord>(
        &self,
        id: &Value,
        exclude_deleted: bool,
    ) -> Result<Option<M>, OfflineDbError> {
        let deleted_filter = if exclude_deleted {
            format!(" AND {} IS NULL", M::DELETED_AT_COLUMN)
        } else {
            String::new()
        };
        let row = self.db.query_optional(
            &format!(
                "SELECT {} FROM {} WHERE {} = ?1{};",
                M::column_names().join(", "),
                M::SQLITE_TABLE,
                M::primary_key_column(),
                deleted_filter,
            ),
            std::slice::from_ref(id),
        )?;

        let Some(row) = row else { return Ok(None) };
        let mut model = M::from_row(&row)?;
        if let Some(ttable) = M::translation_table() {
            let rows = self.db.query(
                &format!(
                    "SELECT {TRANSLATION_COLUMNS} FROM {ttable} WHERE {} = ?1;",
                    M::primary_key_column()
                ),
                std::slice::from_ref(id),
            )?;
            for row in rows.iter().filter_map(|r| translation_row(r, 0)) {
                model.apply_translation(row);
            }
        }
        Ok(Some(model))
    }

    /// Fetches all records, hydrating translations in one batched query.
    #[instrument(level = "debug", skip(self), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn all<M: OfflineRecord>(&self) -> Result<Vec<M>, OfflineDbError> {
        let where_clause = if M::SOFT_DELETE {
            format!(" WHERE {} IS NULL", M::DELETED_AT_COLUMN)
        } else {
            String::new()
        };
        let rows = self.db.query(
            &format!(
                "SELECT {} FROM {}{};",
                M::column_names().join(", "),
                M::SQLITE_TABLE,
                where_clause,
            ),
            &[],
        )?;

        let mut models: Vec<M> = rows.iter().map(M::from_row).collect::<Result<_, _>>()?;
        self.hydrate_translations(&mut models)?;
        Ok(models)
    }

    /// Hydrates the non-source-locale translatable fields of `models` from their own
    /// companion rows, in bounded key batches.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError`] if the companion-table read fails.
    pub fn hydrate_translations<M: OfflineRecord>(
        &self,
        models: &mut [M],
    ) -> Result<(), OfflineDbError> {
        let Some(ttable) = M::translation_table() else {
            return Ok(());
        };
        let pk = M::primary_key_column();
        let keys: Vec<Value> = models.iter().map(primary_key_value).collect();
        let mut by_key: HashMap<KeyOf, Vec<TranslationRow>> = HashMap::new();
        for chunk in keys.chunks(HYDRATE_CHUNK) {
            let rows = self.db.query(
                &format!(
                    "SELECT {pk}, {TRANSLATION_COLUMNS} FROM {ttable} WHERE {pk} IN ({});",
                    placeholders(chunk.len())
                ),
                chunk,
            )?;
            let owned = rows
                .iter()
                .filter_map(|r| Some((r.get(0)?.clone(), translation_row(r, 1)?)));
            for (key, row) in owned {
                by_key.entry(KeyOf(key)).or_default().push(row);
            }
        }
        // Cloned rather than removed: the same record may be given twice.
        for (model, key) in models.iter_mut().zip(keys) {
            for row in by_key.get(&KeyOf(key)).into_iter().flatten() {
                model.apply_translation(row.clone());
            }
        }
        Ok(())
    }

    /// Runs a filtered `SELECT` and returns translation-hydrated models, excluding
    /// soft-deleted rows.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError`] if the query fails or a row cannot be decoded.
    pub fn query_filtered<M: OfflineRecord>(
        &self,
        where_body: &str,
        order_limit: &str,
        params: &[Value],
    ) -> Result<Vec<M>, OfflineDbError> {
        let sql = format!(
            "SELECT {} FROM {}{}{};",
            M::column_names().join(", "),
            M::SQLITE_TABLE,
            self.where_clause::<M>(where_body),
            if order_limit.is_empty() {
                String::new()
            } else {
                format!(" {order_limit}")
            },
        );
        let rows = self.db.query(&sql, params)?;
        let mut models: Vec<M> = rows.iter().map(M::from_row).collect::<Result<_, _>>()?;
        self.hydrate_translations(&mut models)?;
        Ok(models)
    }

    /// Counts rows matching a filter, excluding soft-deleted rows.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError`] if the query fails.
    pub fn count_filtered<M: OfflineRecord>(
        &self,
        where_body: &str,
        params: &[Value],
    ) -> Result<u64, OfflineDbError> {
        let sql = format!(
            "SELECT COUNT(*) FROM {}{};",
            M::SQLITE_TABLE,
            self.where_clause::<M>(where_body),
        );
        let row = self.db.query_one(&sql, params)?;
        Ok(row.get(0).and_then(Value::as_integer).unwrap_or(0).max(0) as u64)
    }

    /// Hard-deletes rows matching a filter and their translation rows, returning how
    /// many main rows were removed.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError`] if a statement fails.
    pub fn delete_filtered<M: OfflineRecord>(
        &self,
        where_body: &str,
        params: &[Value],
    ) -> Result<u64, OfflineDbError> {
        let where_sql = if where_body.is_empty() {
            String::new()
        } else {
            format!(" WHERE {where_body}")
        };
        if let Some(ttable) = M::translation_table() {
            self.db.execute(
                &format!(
                    "DELETE FROM {ttable} WHERE {pk} IN (SELECT {pk} FROM {}{where_sql});",
                    M::SQLITE_TABLE,
                    pk = M::primary_key_column(),
                ),
                params,
            )?;
        }
        let deleted = self.db.execute(
            &format!("DELETE FROM {}{where_sql};", M::SQLITE_TABLE),
            params,
        )?;
        Ok(deleted as u64)
    }

    /// Applies a rendered `SET` list to rows matching a filter, returning how many were
    /// updated; `params` bind the `SET` placeholders first.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError`] if the statement fails.
    pub fn update_filtered<M: OfflineRecord>(
        &self,
        set_body: &str,
        where_body: &str,
        params: &[Value],
    ) -> Result<u64, OfflineDbError> {
        let sql = format!(
            "UPDATE {} SET {set_body}{};",
            M::SQLITE_TABLE,
            self.where_clause::<M>(where_body),
        );
        let updated = self.db.execute(&sql, params)?;
        Ok(updated as u64)
    }

    /// The soft-delete exclusion `AND`-ed with the caller's filter body.
    fn where_clause<M: OfflineModel>(&self, where_body: &str) -> String {
        let mut conditions: Vec<String> = Vec::new();
        if M::SOFT_DELETE {
            conditions.push(format!("{} IS NULL", M::DELETED_AT_COLUMN));
        }
        if !where_body.is_empty() {
            conditions.push(where_body.to_string());
        }
        if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        }
    }

    /// Deletes a record and its translation rows; returns whether a row was removed.
    #[instrument(skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn delete<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        let main_sql = format!(
            "DELETE FROM {} WHERE {} = ?1;",
            M::SQLITE_TABLE,
            M::primary_key_column()
        );
        let removed = match M::translation_table() {
            None => self.db.execute(&main_sql, std::slice::from_ref(id))? > 0,
            Some(ttable) => {
                let tx = self.db.transaction()?;
                tx.execute(
                    &format!(
                        "DELETE FROM {ttable} WHERE {} = ?1;",
                        M::primary_key_column()
                    ),
                    std::slice::from_ref(id),
                )?;
                let changed = tx.execute(&main_sql, std::slice::from_ref(id))?;
                tx.commit()?;
                changed > 0
            }
        };
        if removed {
            self.emit::<M>(id.clone(), ChangeOp::Delete);
        }
        Ok(removed)
    }

    /// Soft-deletes a record by stamping `deleted_at`; returns whether a row was affected.
    #[instrument(skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn soft_delete<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        let sql = format!(
            "UPDATE {} SET {} = datetime('now') WHERE {} = ?1 AND {} IS NULL;",
            M::SQLITE_TABLE,
            M::DELETED_AT_COLUMN,
            M::primary_key_column(),
            M::DELETED_AT_COLUMN,
        );
        let affected = self.db.execute(&sql, std::slice::from_ref(id))? > 0;
        if affected {
            self.emit::<M>(id.clone(), ChangeOp::Delete);
        }
        Ok(affected)
    }

    /// Restores a soft-deleted record; returns whether a row was affected.
    #[instrument(skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn restore<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        let sql = format!(
            "UPDATE {} SET {} = NULL WHERE {} = ?1;",
            M::SQLITE_TABLE,
            M::DELETED_AT_COLUMN,
            M::primary_key_column(),
        );
        let affected = self.db.execute(&sql, std::slice::from_ref(id))? > 0;
        if affected {
            self.emit::<M>(id.clone(), ChangeOp::Update);
        }
        Ok(affected)
    }

    /// Fetches a record by primary key, including soft-deleted rows.
    #[instrument(level = "debug", skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn find_with_deleted<M: OfflineRecord>(
        &self,
        id: &Value,
    ) -> Result<Option<M>, OfflineDbError> {
        self.get_inner::<M>(id, false)
    }

    /// Permanently removes a record and its translation rows.
    #[instrument(skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn purge<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        self.delete::<M>(id)
    }

    /// Counts all rows in the model's table.
    #[instrument(level = "debug", skip(self), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn count<M: OfflineModel>(&self) -> Result<u64, OfflineDbError> {
        let row = self
            .db
            .query_one(&format!("SELECT COUNT(*) FROM {};", M::SQLITE_TABLE), &[])?;
        Ok(row.get(0).and_then(Value::as_integer).unwrap_or(0) as u64)
    }

    /// Returns whether a record with the given primary key exists.
    #[instrument(level = "debug", skip(self, id), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn exists<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        let row = self.db.query_optional(
            &format!(
                "SELECT 1 FROM {} WHERE {} = ?1 LIMIT 1;",
                M::SQLITE_TABLE,
                M::primary_key_column()
            ),
            std::slice::from_ref(id),
        )?;
        Ok(row.is_some())
    }

    /// Finds the primary keys of records whose translatable `column` matches the `LIKE`
    /// `pattern` in `locale`.
    #[instrument(level = "debug", skip(self), fields(table = M::SQLITE_TABLE), err(Debug))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn find_ids_by_translation<M: OfflineRecord>(
        &self,
        column: &str,
        locale: &str,
        pattern: &str,
    ) -> Result<Vec<Value>, OfflineDbError> {
        let Some(ttable) = M::translation_table() else {
            return Ok(Vec::new());
        };
        let rows = self.db.query(
            &format!(
                "SELECT DISTINCT {} FROM {ttable} \
                 WHERE field = ?1 AND locale = ?2 AND value LIKE ?3;",
                M::primary_key_column()
            ),
            &[
                Value::from(column.to_string()),
                Value::from(canonical_locale(locale)),
                Value::from(pattern.to_string()),
            ],
        )?;
        Ok(rows.iter().filter_map(|r| r.get(0).cloned()).collect())
    }

    fn ensure_schema_table(&self) -> Result<(), OfflineDbError> {
        self.db.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {SCHEMA_TABLE} (\n    \
                 table_name TEXT NOT NULL PRIMARY KEY,\n    \
                 schema_version INTEGER NOT NULL\n\
             );"
        ))
    }
}

/// Extracts a model's primary-key [`Value`].
pub fn primary_key_value<M: OfflineRecord>(model: &M) -> Value {
    let pk = M::primary_key_column();
    let idx = M::column_names().iter().position(|c| *c == pk).unwrap_or(0);
    model
        .to_values()
        .into_iter()
        .nth(idx)
        .unwrap_or(Value::Null)
}

fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Replaces a record's companion rows; run inside the caller's transaction.
fn replace_translations<M: OfflineRecord>(
    tx: &crate::db::OfflineTransaction<'_>,
    table: &str,
    key: &Value,
    rows: &[TranslationRow],
) -> Result<(), OfflineDbError> {
    let pk = M::primary_key_column();
    tx.execute(
        &format!("DELETE FROM {table} WHERE {pk} = ?1;"),
        std::slice::from_ref(key),
    )?;
    for row in rows {
        tx.execute(
            &format!(
                "INSERT INTO {table} ({pk}, {TRANSLATION_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);"
            ),
            &[
                key.clone(),
                Value::from(row.field.clone()),
                Value::from(row.locale.clone()),
                Value::from(row.value.clone()),
                Value::from(row.check.source_hash().to_string()),
                Value::from(row.check.translation_hash().to_string()),
                row.check
                    .checked()
                    .map_or(Value::Null, |checked| Value::from(checked.by().to_string())),
                row.check
                    .checked()
                    .map_or(Value::Null, |checked| Value::from(checked.at_millis())),
            ],
        )?;
    }
    Ok(())
}

/// How many keys one companion read binds: well under SQLite's bound-parameter limit.
const HYDRATE_CHUNK: usize = 500;

const TRANSLATION_COLUMNS: &str =
    "field, locale, value, source_hash, translation_hash, checked_by, checked_at";

/// The companion row read from column `first` of `row`; `None` when a part is absent.
fn translation_row(row: &crate::db::Row, first: usize) -> Option<TranslationRow> {
    let text = |at: usize| row.get(first + at).and_then(Value::as_text);
    Some(TranslationRow {
        field: text(0)?.to_string(),
        locale: text(1)?.to_string(),
        value: text(2)?.to_string(),
        check: TranslationCheck::stored(
            text(3).unwrap_or(""),
            text(4).unwrap_or(""),
            text(5)
                .zip(row.get(first + 6).and_then(Value::as_integer))
                .map(|(by, at)| Checked::new(by, at)),
        ),
    })
}

fn canonical_locale(locale: &str) -> String {
    locale
        .parse::<fundamental_core::i18n::Locale>()
        .map_or_else(|_| locale.to_string(), |l| l.to_string())
}
