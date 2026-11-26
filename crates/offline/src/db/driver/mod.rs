//! Platform-abstracted SQLite driver.
//!
//! One rusqlite implementation everywhere; only persistent open and export/import
//! differ, in [`platform`].

mod platform;

use std::sync::Arc;

use rusqlite::Connection;
use tracing::{debug, instrument};

use super::{OfflineConfig, OfflineDbError, OfflineLocation, Row, Value};

/// SQLite driver over a rusqlite connection.
#[derive(Debug)]
pub(crate) struct Driver {
    conn: Connection,
    platform: platform::Platform,
}

impl Driver {
    #[instrument(skip(config), fields(location = ?config.location), err(Debug))]
    pub(crate) async fn open(config: &OfflineConfig) -> Result<Self, OfflineDbError> {
        let (conn, platform) = match &config.location {
            OfflineLocation::Memory => {
                debug!("opening in-memory offline database");
                (Connection::open_in_memory()?, platform::Platform::memory())
            }
            OfflineLocation::Persistent(name) => {
                debug!(db = %name, "opening persistent offline database");
                platform::open_persistent(config, name).await?
            }
        };

        Ok(Self { conn, platform })
    }

    pub(crate) fn execute(&self, sql: &str, params: &[Value]) -> Result<usize, OfflineDbError> {
        // `?` keeps the typed rusqlite error and its extended result code.
        let mut stmt = self.conn.prepare(sql)?;
        let affected = stmt.execute(rusqlite::params_from_iter(params.iter().map(to_sql_value)))?;
        Ok(affected)
    }

    pub(crate) fn execute_batch(&self, sql: &str) -> Result<(), OfflineDbError> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    pub(crate) fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, OfflineDbError> {
        let mut stmt = self.conn.prepare(sql)?;

        let columns: Arc<[String]> = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        let column_count = columns.len();

        let mut rows = stmt.query(rusqlite::params_from_iter(params.iter().map(to_sql_value)))?;

        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let value: rusqlite::types::Value = row.get(i)?;
                values.push(from_sql_value(value));
            }
            result.push(Row::new(Arc::clone(&columns), values));
        }

        Ok(result)
    }

    /// Exports the database as SQLite file bytes.
    pub(crate) fn export_bytes(&self) -> Result<Vec<u8>, OfflineDbError> {
        platform::export_bytes(&self.conn, &self.platform)
    }

    /// Imports SQLite file bytes; must be called before `open`.
    pub(crate) async fn import_bytes(
        config: &OfflineConfig,
        bytes: &[u8],
    ) -> Result<(), OfflineDbError> {
        platform::import_bytes(config, bytes).await
    }

    pub(crate) fn sqlite_version_number(&self) -> i32 {
        rusqlite::version_number()
    }

    pub(crate) fn sqlite_version(&self) -> String {
        rusqlite::version().to_string()
    }
}

fn to_sql_value(value: &Value) -> rusqlite::types::Value {
    match value {
        Value::Null => rusqlite::types::Value::Null,
        Value::Integer(i) => rusqlite::types::Value::Integer(*i),
        Value::Real(r) => rusqlite::types::Value::Real(*r),
        Value::Text(t) => rusqlite::types::Value::Text(t.clone()),
        Value::Blob(b) => rusqlite::types::Value::Blob(b.clone()),
    }
}

fn from_sql_value(value: rusqlite::types::Value) -> Value {
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(i) => Value::Integer(i),
        rusqlite::types::Value::Real(r) => Value::Real(r),
        rusqlite::types::Value::Text(t) => Value::Text(t),
        rusqlite::types::Value::Blob(b) => Value::Blob(b),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[tokio::test]
    async fn constraint_violation_preserves_the_typed_sqlite_code() {
        // A UNIQUE violation surfaces as `OfflineDbError::Sqlite`, not a flattened string.
        let driver = Driver::open(&OfflineConfig::memory())
            .await
            .expect("open in-memory db");
        driver
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
            .expect("create table");
        driver
            .execute("INSERT INTO t (id) VALUES (1)", &[])
            .expect("first insert");

        let err = driver
            .execute("INSERT INTO t (id) VALUES (1)", &[])
            .expect_err("duplicate primary key must fail");

        match err {
            OfflineDbError::Sqlite(ref e) => {
                assert_eq!(
                    e.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::ConstraintViolation),
                    "the SQLite constraint code must be preserved"
                );
            }
            other => panic!("expected OfflineDbError::Sqlite, got {other:?}"),
        }
        assert!(
            err.source().is_some(),
            "Sqlite variant must expose its source"
        );
    }
}
