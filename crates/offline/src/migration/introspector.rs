//! Reading a live SQLite schema back into a [`DatabaseSchema`].
//!
//! Columns and the primary key only; native-only, since it drives migration generation.

use fundamental_db::DatabaseResult;
use fundamental_db::migration::{
    DatabaseSchema, MigrationDatabase, SchemaIntrospector, TableColumn, TableSchema,
};

use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use crate::db::{OfflineDatabase, OfflineDbError};

const MIGRATIONS_TABLE: &str = "_fundamental_migrations";

/// Whether a table is the framework's migration tracking; `<table>_i18n` companions are compared.
fn is_framework_table(name: &str) -> bool {
    name == MIGRATIONS_TABLE
}

/// Reads the live schema of an offline SQLite database.
pub struct OfflineIntrospector;

impl SchemaIntrospector for OfflineIntrospector {
    type Conn = OfflineDatabase;

    async fn introspect(conn: &OfflineDatabase) -> DatabaseResult<DatabaseSchema> {
        Ok(read_schema(conn)?)
    }
}

/// Reads every user table, skipping SQLite internals and the migration-tracking table.
#[instrument(skip(conn), fields(tables = Empty), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn read_schema(conn: &OfflineDatabase) -> Result<DatabaseSchema, OfflineDbError> {
    let mut schema = DatabaseSchema::for_database(MigrationDatabase::OfflineDb);

    let table_rows = conn.query(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
        &[],
    )?;

    for row in &table_rows {
        let name = row
            .get_named("name")
            .and_then(|v| v.as_text())
            .ok_or_else(|| {
                OfflineDbError::Introspection("sqlite_master row missing a text `name`".to_string())
            })?
            .to_string();
        if is_framework_table(&name) {
            continue;
        }
        schema.add_table(read_table(conn, &name)?);
    }

    Span::current().record("tables", schema.tables.len());
    debug!(tables = schema.tables.len(), "read offline schema");
    Ok(schema)
}

/// Reads one table's columns and primary key via `PRAGMA table_info`.
fn read_table(conn: &OfflineDatabase, table: &str) -> Result<TableSchema, OfflineDbError> {
    let mut schema = TableSchema::new(table);
    // A PRAGMA argument cannot be bound, so quote defensively.
    let quoted = format!("\"{}\"", table.replace('"', "\"\""));
    let columns = conn.query(&format!("PRAGMA table_info({quoted})"), &[])?;

    // `PRAGMA table_info` columns: cid, name, type, notnull, dflt_value, pk.
    let mut primary_key: Vec<(i64, String)> = Vec::new();
    for row in &columns {
        let name = row
            .get_named("name")
            .and_then(|v| v.as_text())
            .ok_or_else(|| {
                OfflineDbError::Introspection(format!("table_info({table}) row missing `name`"))
            })?
            .to_string();
        let data_type = row
            .get_named("type")
            .and_then(|v| v.as_text())
            .unwrap_or("")
            .to_string();
        if data_type.is_empty() {
            warn!(table, column = %name, "introspected column has no declared type");
        }
        let not_null = row
            .get_named("notnull")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let pk_index = row
            .get_named("pk")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        let default = row
            .get_named("dflt_value")
            .and_then(|v| v.as_text())
            .map(|s| s.to_string());

        if pk_index > 0 {
            primary_key.push((pk_index, name.clone()));
        }
        schema.add_column(TableColumn {
            name,
            data_type,
            nullable: not_null == 0,
            default,
            primary_key: pk_index > 0,
            unique: false,
        });
    }

    primary_key.sort_by_key(|(index, _)| *index);
    if !primary_key.is_empty() {
        schema.set_primary_key(primary_key.into_iter().map(|(_, name)| name).collect());
    }
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::OfflineConfig;

    async fn open() -> OfflineDatabase {
        OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("in-memory database opens")
    }

    #[tokio::test]
    async fn introspects_columns_and_primary_key() {
        let db = open().await;
        db.execute_batch(
            "CREATE TABLE widgets (\
                id TEXT NOT NULL,\
                revision INTEGER NOT NULL,\
                note TEXT,\
                PRIMARY KEY (id))",
        )
        .expect("create table");

        let schema = read_schema(&db).expect("introspect");
        let table = schema.get_table("widgets").expect("widgets table present");

        assert_eq!(table.primary_key, vec!["id".to_string()]);

        let id = table.columns.get("id").expect("id column");
        assert_eq!(id.data_type, "TEXT");
        assert!(!id.nullable);
        assert!(id.primary_key);

        let revision = table.columns.get("revision").expect("revision column");
        assert_eq!(revision.data_type, "INTEGER");
        assert!(!revision.nullable);

        let note = table.columns.get("note").expect("note column");
        assert!(note.nullable);
    }

    #[tokio::test]
    async fn excludes_the_migration_tracking_table() {
        let db = open().await;
        db.execute_batch(&format!(
            "CREATE TABLE {MIGRATIONS_TABLE} (name TEXT PRIMARY KEY, applied_at TEXT)"
        ))
        .expect("create tracking table");
        db.execute_batch("CREATE TABLE real_table (id TEXT PRIMARY KEY)")
            .expect("create real table");

        let schema = read_schema(&db).expect("introspect");
        assert!(schema.get_table("real_table").is_some());
        assert!(
            schema.get_table(MIGRATIONS_TABLE).is_none(),
            "the framework's tracking table must never appear as schema drift"
        );
    }
}
