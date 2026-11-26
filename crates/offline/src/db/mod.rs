//! Unified offline SQLite database.
//!
//! rusqlite with bundled SQLite natively, sqlite-wasm-rs over IndexedDB or OPFS in the
//! browser; both ship SQLite ≥ 3.53, which [`OfflineDatabase::open`] enforces.

mod config;
mod driver;
mod error;
mod row;
mod value;

pub use config::{OfflineConfig, OfflineLocation, WasmBackend};
pub use error::OfflineDbError;
pub use row::Row;
pub use value::Value;

use tracing::field::Empty;
use tracing::{Span, debug, error, info, instrument, warn};

use driver::Driver;
use futures::channel::mpsc::UnboundedReceiver;

use crate::change::{LocalChangeBus, TableChange};
use crate::store::{OfflineRecord, OfflineStore, primary_key_value};
use fundamental_core::model::OfflineModel;

/// Minimum supported SQLite version (3.53.0).
pub const MIN_SQLITE_VERSION: i32 = 3_053_000;

/// A platform-abstracted offline SQLite database.
#[derive(Debug)]
pub struct OfflineDatabase {
    driver: Driver,
    changes: LocalChangeBus,
}

impl OfflineDatabase {
    /// Opens the database described by `config`.
    #[instrument(
        skip(config),
        fields(
            persistent = matches!(config.location, OfflineLocation::Persistent(_)),
            foreign_keys = config.foreign_keys,
            sqlite_version = Empty,
        )
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn open(config: OfflineConfig) -> Result<Self, OfflineDbError> {
        let driver = Driver::open(&config).await?;
        let db = Self {
            driver,
            changes: LocalChangeBus::default(),
        };

        let version = db.sqlite_version_number();
        let version_str = db.sqlite_version();
        Span::current().record("sqlite_version", version_str.as_str());
        if version < MIN_SQLITE_VERSION {
            error!(
                found = %version_str,
                required = "3.53.0",
                "offline database rejected: SQLite version below minimum"
            );
            return Err(OfflineDbError::UnsupportedVersion {
                found: version_str,
                required: "3.53.0".to_string(),
            });
        }

        // Set in both directions: bundled SQLite defaults foreign keys ON.
        debug!(
            foreign_keys = config.foreign_keys,
            "applying foreign_keys pragma from config"
        );
        db.execute_batch(if config.foreign_keys {
            "PRAGMA foreign_keys=ON;"
        } else {
            "PRAGMA foreign_keys=OFF;"
        })?;

        info!("offline database opened");
        Ok(db)
    }

    /// Executes a single statement, returning the number of affected rows.
    #[instrument(level = "debug", skip(self, params), fields(sql, params = params.len()))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<usize, OfflineDbError> {
        self.driver.execute(sql, params)
    }

    /// Executes multiple statements (no parameters).
    #[instrument(level = "debug", skip(self, sql))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn execute_batch(&self, sql: &str) -> Result<(), OfflineDbError> {
        self.driver.execute_batch(sql)
    }

    /// Runs a query and returns all rows.
    #[instrument(level = "debug", skip(self, params), fields(sql, params = params.len(), rows = Empty))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, OfflineDbError> {
        let rows = self.driver.query(sql, params)?;
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Runs a query expected to return at most one row.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn query_optional(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<Option<Row>, OfflineDbError> {
        Ok(self.query(sql, params)?.into_iter().next())
    }

    /// Runs a query expected to return exactly one row.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn query_one(&self, sql: &str, params: &[Value]) -> Result<Row, OfflineDbError> {
        self.query_optional(sql, params)?
            .ok_or(OfflineDbError::NoRows)
    }

    /// Begins a transaction.
    ///
    /// The transaction rolls back when dropped unless
    /// [`OfflineTransaction::commit`] is called.
    #[instrument(level = "debug", skip(self))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn transaction(&self) -> Result<OfflineTransaction<'_>, OfflineDbError> {
        self.execute_batch("BEGIN;")?;
        debug!("offline transaction begun");
        Ok(OfflineTransaction {
            db: self,
            finished: false,
        })
    }

    /// Exports the database as SQLite file bytes.
    #[instrument(level = "debug", skip(self), fields(bytes = Empty))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn export_bytes(&self) -> Result<Vec<u8>, OfflineDbError> {
        let bytes = self.driver.export_bytes()?;
        Span::current().record("bytes", bytes.len());
        Ok(bytes)
    }

    /// Imports SQLite file bytes into the location described by `config`.
    ///
    /// Must be called **before** [`open`](Self::open); overwrites any
    /// existing database at that location.
    #[instrument(level = "debug", skip(config, bytes), fields(bytes = bytes.len()))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn import_bytes(config: &OfflineConfig, bytes: &[u8]) -> Result<(), OfflineDbError> {
        Driver::import_bytes(config, bytes).await
    }

    /// Returns the SQLite library version string (e.g. "3.53.1").
    pub fn sqlite_version(&self) -> String {
        self.driver.sqlite_version()
    }

    /// Returns the SQLite library version number (e.g. 3053001).
    pub fn sqlite_version_number(&self) -> i32 {
        self.driver.sqlite_version_number()
    }

    /// Returns a stream of every change to `table`.
    #[must_use]
    pub fn watch(&self, table: impl Into<String>) -> UnboundedReceiver<TableChange> {
        self.changes.watch(table)
    }

    /// Emits `change` to the watchers of its table.
    pub fn notify(&self, change: TableChange) {
        self.changes.notify(change);
    }
}

/// An open transaction.
///
/// Rolls back on drop unless [`commit`](Self::commit) is called.
#[derive(Debug)]
pub struct OfflineTransaction<'a> {
    db: &'a OfflineDatabase,
    finished: bool,
}

impl OfflineTransaction<'_> {
    /// Executes a single statement within the transaction.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<usize, OfflineDbError> {
        self.db.execute(sql, params)
    }

    /// Executes multiple statements within the transaction.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn execute_batch(&self, sql: &str) -> Result<(), OfflineDbError> {
        self.db.execute_batch(sql)
    }

    /// Runs a query within the transaction.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, OfflineDbError> {
        self.db.query(sql, params)
    }

    /// Commits the transaction.
    #[instrument(level = "debug", skip(self))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn commit(mut self) -> Result<(), OfflineDbError> {
        self.finished = true;
        self.db.execute_batch("COMMIT;")?;
        debug!("offline transaction committed");
        Ok(())
    }

    /// Rolls back the transaction.
    #[instrument(level = "debug", skip(self))]
    #[expect(
        clippy::cognitive_complexity,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub fn rollback(mut self) -> Result<(), OfflineDbError> {
        self.finished = true;
        self.db.execute_batch("ROLLBACK;")?;
        debug!("offline transaction rolled back");
        Ok(())
    }
}

/// Typed, model-aware CRUD **inside** an offline transaction.
/// Typed CRUD inside the transaction, through the synchronous [`OfflineStore`] on the same connection.
impl OfflineTransaction<'_> {
    /// Inserts a model within the transaction, returning it.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the insert fails.
    pub fn insert<M: OfflineRecord + Clone>(&self, model: &M) -> Result<M, OfflineDbError> {
        OfflineStore::new(self.db).insert(model)?;
        Ok(model.clone())
    }

    /// Updates a model within the transaction, returning it.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the update fails.
    pub fn update<M: OfflineRecord + Clone>(&self, model: &M) -> Result<M, OfflineDbError> {
        OfflineStore::new(self.db).upsert(model)?;
        Ok(model.clone())
    }

    /// Inserts or updates a model (upsert) within the transaction, returning it.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the upsert fails.
    pub fn upsert<M: OfflineRecord + Clone>(&self, model: &M) -> Result<M, OfflineDbError> {
        OfflineStore::new(self.db).upsert(model)?;
        Ok(model.clone())
    }

    /// Finds a model by its ID within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn find<M: OfflineRecord>(&self, id: &Value) -> Result<Option<M>, OfflineDbError> {
        OfflineStore::new(self.db).get::<M>(id)
    }

    /// Finds a model by its ID within the transaction, erroring if not found.
    ///
    /// # Errors
    ///
    /// Returns [`OfflineDbError::NoRows`] if no row matches, or another
    /// [`OfflineDbError`] if the query fails.
    pub fn find_required<M: OfflineRecord>(&self, id: &Value) -> Result<M, OfflineDbError> {
        OfflineStore::new(self.db)
            .get::<M>(id)?
            .ok_or(OfflineDbError::NoRows)
    }

    /// Checks whether a model with the given ID exists within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn exists<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        OfflineStore::new(self.db).exists::<M>(id)
    }

    /// Deletes a model by its ID within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the delete fails.
    pub fn delete<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        OfflineStore::new(self.db).delete::<M>(id)
    }

    /// Finds multiple models by their IDs within the transaction; missing IDs are
    /// skipped.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if a query fails.
    pub fn find_many<M: OfflineRecord>(&self, ids: &[Value]) -> Result<Vec<M>, OfflineDbError> {
        let store = OfflineStore::new(self.db);
        let mut out = Vec::new();
        for id in ids {
            if let Some(model) = store.get::<M>(id)? {
                out.push(model);
            }
        }
        Ok(out)
    }

    /// Inserts multiple models within the transaction, returning them.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if an insert fails.
    pub fn insert_many<M: OfflineRecord + Clone>(
        &self,
        models: &[M],
    ) -> Result<Vec<M>, OfflineDbError> {
        let store = OfflineStore::new(self.db);
        for model in models {
            store.insert(model)?;
        }
        Ok(models.to_vec())
    }

    /// Updates multiple models within the transaction, returning them.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if an update fails.
    pub fn update_many<M: OfflineRecord + Clone>(
        &self,
        models: &[M],
    ) -> Result<Vec<M>, OfflineDbError> {
        let store = OfflineStore::new(self.db);
        for model in models {
            store.upsert(model)?;
        }
        Ok(models.to_vec())
    }

    /// Deletes multiple models by their IDs within the transaction, returning the
    /// number removed.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if a delete fails.
    pub fn delete_many<M: OfflineRecord>(&self, ids: &[Value]) -> Result<u64, OfflineDbError> {
        let store = OfflineStore::new(self.db);
        let mut deleted = 0u64;
        for id in ids {
            if store.delete::<M>(id)? {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Counts all rows of `M` within the transaction (soft-deleted rows excluded).
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn count<M: OfflineModel>(&self) -> Result<u64, OfflineDbError> {
        OfflineStore::new(self.db).count::<M>()
    }

    /// Soft-deletes a model by its ID within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn soft_delete<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        OfflineStore::new(self.db).soft_delete::<M>(id)
    }

    /// Restores a soft-deleted model within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn restore<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        OfflineStore::new(self.db).restore::<M>(id)
    }

    /// Finds a model by its ID including soft-deleted rows, within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn find_with_deleted<M: OfflineRecord>(
        &self,
        id: &Value,
    ) -> Result<Option<M>, OfflineDbError> {
        OfflineStore::new(self.db).find_with_deleted::<M>(id)
    }

    /// Permanently deletes a model by its ID (hard delete) within the transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn purge<M: OfflineRecord>(&self, id: &Value) -> Result<bool, OfflineDbError> {
        OfflineStore::new(self.db).purge::<M>(id)
    }

    /// Finds a model by ID within the transaction, or inserts the one produced by
    /// `create` if absent.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if a query fails.
    pub fn find_or_create<M, F>(&self, id: &Value, create: F) -> Result<M, OfflineDbError>
    where
        M: OfflineRecord + Clone,
        F: FnOnce() -> M,
    {
        match self.find::<M>(id)? {
            Some(found) => Ok(found),
            None => self.insert(&create()),
        }
    }

    /// Updates a model within the transaction only if it already exists.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if a query fails.
    pub fn update_if_exists<M: OfflineRecord + Clone>(
        &self,
        model: &M,
    ) -> Result<Option<M>, OfflineDbError> {
        let id = primary_key_value(model);
        if self.exists::<M>(&id)? {
            Ok(Some(self.update(model)?))
        } else {
            Ok(None)
        }
    }

    /// Finds ids whose translatable `field` matches `value` in `locale`, within the
    /// transaction.
    ///
    /// # Errors
    ///
    /// Returns an [`OfflineDbError`] if the query fails.
    pub fn find_ids_by_translation<M: OfflineRecord>(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> Result<Vec<Value>, OfflineDbError> {
        OfflineStore::new(self.db).find_ids_by_translation::<M>(field, locale, value)
    }
}

impl Drop for OfflineTransaction<'_> {
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn drop(&mut self) {
        if !self.finished {
            // Drop cannot return a Result, so the log is the only signal.
            match self.db.execute_batch("ROLLBACK;") {
                Ok(()) => {
                    debug!("offline transaction dropped without commit; rolled back")
                }
                Err(error) => {
                    warn!(%error, "offline transaction drop rollback failed")
                }
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[tokio::test]
    async fn dropping_uncommitted_transaction_logs_implicit_rollback() {
        let db = OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("open in-memory db");
        db.execute_batch("CREATE TABLE a (id INTEGER PRIMARY KEY);")
            .expect("create table");

        {
            let tx = db.transaction().expect("begin");
            tx.execute("INSERT INTO a (id) VALUES (1);", &[])
                .expect("insert");
        }

        assert!(
            logs_contain("dropped without commit"),
            "an implicit rollback on drop must be logged"
        );
        let rows = db.query("SELECT id FROM a;", &[]).expect("query");
        assert!(rows.is_empty(), "the rolled-back row must not survive");
    }
}
