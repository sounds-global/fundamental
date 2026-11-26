//! Reading a live YCQL schema back into a [`DatabaseSchema`].
//!
//! CQL has no column nullability, so every column is reported nullable, as the derive records it.

use fundamental_db::DatabaseResult;
use fundamental_db::migration::{
    DatabaseSchema, MigrationDatabase, SchemaIntrospector, TableColumn, TableSchema,
    is_ledger_table,
};

use fundamental_core::model::cql::ClusteringOrder;
use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use crate::pool::CqlPool;
use crate::{CqlError, CqlValue, cql};

/// The most `system_schema.columns` rows one keyspace may have before
/// introspection refuses.
///
/// Reaching it means something is wrong that a silent multi-second read would hide.
const MAX_SCHEMA_COLUMNS: usize = 10_000;

/// Reads the live schema of a YCQL keyspace.
pub struct CqlIntrospector;

impl SchemaIntrospector for CqlIntrospector {
    type Conn = CqlPool;

    async fn introspect(conn: &CqlPool) -> DatabaseResult<DatabaseSchema> {
        Ok(read_schema(conn).await?)
    }
}

/// Whether a table is the migration ledger's own; a model's `<table>_i18n` companion is not.
fn is_framework_table(name: &str) -> bool {
    is_ledger_table(name)
}

/// One column as reported by `system_schema.columns`.
struct ColumnRow {
    table: String,
    column: String,
    cql_type: String,
    kind: String,
    position: i32,
    clustering_order: String,
}

/// In-progress per-table accumulation while folding the column rows.
#[derive(Default)]
struct TableBuilder {
    columns: Vec<TableColumn>,
    partition: Vec<(i32, String)>,
    clustering: Vec<(i32, String, ClusteringOrder)>,
}

#[instrument(skip(conn), fields(keyspace = Empty, tables = Empty), err(Debug))]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn read_schema(conn: &CqlPool) -> Result<DatabaseSchema, CqlError> {
    let keyspace = conn.session().inner().get_keyspace().ok_or_else(|| {
        CqlError::Introspection(
            "the session has no keyspace selected; introspection needs a keyspace".to_string(),
        )
    })?;
    let keyspace = keyspace.as_str();
    Span::current().record("keyspace", keyspace);

    let rows = fetch_columns(conn, keyspace).await?;

    let mut tables: std::collections::HashMap<String, TableBuilder> =
        std::collections::HashMap::new();
    for row in rows {
        if is_framework_table(&row.table) {
            continue;
        }
        let builder = tables.entry(row.table.clone()).or_default();
        match row.kind.as_str() {
            "partition_key" => builder.partition.push((row.position, row.column.clone())),
            "clustering" => {
                let order = match row.clustering_order.to_ascii_lowercase().as_str() {
                    "desc" => ClusteringOrder::Desc,
                    _ => ClusteringOrder::Asc,
                };
                builder
                    .clustering
                    .push((row.position, row.column.clone(), order));
            }
            "regular" | "static" => {}
            other => {
                warn!(table = %row.table, column = %row.column, kind = other, "unknown column kind")
            }
        }
        builder.columns.push(TableColumn {
            name: row.column,
            data_type: row.cql_type,
            // CQL has no per-column nullability; match the derive's YCQL schema.
            nullable: true,
            default: None,
            primary_key: matches!(row.kind.as_str(), "partition_key" | "clustering"),
            unique: false,
        });
    }

    let mut schema = DatabaseSchema::for_database(MigrationDatabase::CqlDb);
    for (name, mut builder) in tables {
        let mut table = TableSchema::new(&name);
        // Partition keys, then clustering keys, then the rest; column order means nothing in CQL.
        builder.partition.sort_by_key(|(pos, _)| *pos);
        builder.clustering.sort_by_key(|(pos, _, _)| *pos);
        let key_names: Vec<String> = builder
            .partition
            .iter()
            .map(|(_, n)| n.clone())
            .chain(builder.clustering.iter().map(|(_, n, _)| n.clone()))
            .collect();
        for key in &key_names {
            if let Some(col) = builder.columns.iter().find(|c| &c.name == key) {
                table.add_column(col.clone());
            }
        }
        for col in &builder.columns {
            if !key_names.contains(&col.name) {
                table.add_column(col.clone());
            }
        }
        if !builder.partition.is_empty() {
            table.set_partition_key(builder.partition.iter().map(|(_, n)| n.clone()).collect());
        }
        if !builder.clustering.is_empty() {
            table.set_clustering_key(
                builder
                    .clustering
                    .iter()
                    .map(|(_, n, o)| (n.clone(), *o))
                    .collect(),
            );
        }
        if !key_names.is_empty() {
            table.set_primary_key(key_names);
        }
        schema.add_table(table);
    }

    Span::current().record("tables", schema.tables.len());
    debug!(tables = schema.tables.len(), "read ycql schema");
    Ok(schema)
}

/// Runs the `system_schema.columns` query for one keyspace.
async fn fetch_columns(conn: &CqlPool, keyspace: &str) -> Result<Vec<ColumnRow>, CqlError> {
    let statement = cql!(
        "SELECT table_name, column_name, type, kind, position, clustering_order \
         FROM system_schema.columns WHERE keyspace_name = ?"
    )
    .bind(CqlValue::text(keyspace))
    .idempotent();

    Ok(conn
        .fetch_paged::<(String, String, String, String, i32, String)>(
            &statement,
            MAX_SCHEMA_COLUMNS,
        )
        .await?
        .into_iter()
        .map(
            |(table, column, cql_type, kind, position, clustering_order)| ColumnRow {
                table,
                column,
                cql_type,
                kind,
                position,
                clustering_order,
            },
        )
        .collect())
}
