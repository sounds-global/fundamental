//! Reading a live YSQL schema back into a [`DatabaseSchema`], with types normalized to the derive's names.

use fundamental_core::model::sql::ShardKind;
use fundamental_db::DatabaseResult;
use fundamental_db::migration::ddl::{KeyDeclaration, Sharding};
use fundamental_db::migration::{
    DatabaseSchema, KeyPart, MigrationDatabase, SchemaIntrospector, TableCheckConstraint,
    TableColumn, TableIndex, TableSchema, is_ledger_table,
};

use sqlx::Row as _;
use tracing::field::Empty;
use tracing::{Span, debug, instrument, warn};

use crate::SqlError;
use crate::pool::SqlPool;

/// Reads the live schema of a YSQL database.
pub struct SqlIntrospector;

impl SchemaIntrospector for SqlIntrospector {
    type Conn = SqlPool;

    async fn introspect(conn: &SqlPool) -> DatabaseResult<DatabaseSchema> {
        Ok(read_schema(conn).await?)
    }
}

impl SqlIntrospector {
    /// Every index of the connection's schema the planner never uses, as `(table, index)`: an unfinished build keeps its name.
    ///
    /// # Errors
    /// [`SqlError`] when the catalog cannot be read.
    #[instrument(skip(conn), fields(unusable = Empty), err(Debug))]
    pub async fn unusable_indexes(conn: &SqlPool) -> Result<Vec<(String, String)>, SqlError> {
        let rows = sqlx::query(
            "SELECT t.relname::text AS table_name, c.relname::text AS index_name \
             FROM pg_index i \
             JOIN pg_class c ON c.oid = i.indexrelid \
             JOIN pg_class t ON t.oid = i.indrelid \
             WHERE t.relnamespace = current_schema()::regnamespace \
               AND NOT (i.indisvalid AND i.indisready AND i.indislive) \
             ORDER BY 1, 2",
        )
        .fetch_all(&conn.inner())
        .await?;
        let unusable = rows
            .iter()
            .map(|row| {
                let table: String = row
                    .try_get("table_name")
                    .map_err(|e| SqlError::Introspection(format!("missing table_name: {e}")))?;
                let index: String = row
                    .try_get("index_name")
                    .map_err(|e| SqlError::Introspection(format!("missing index_name: {e}")))?;
                Ok((table, index))
            })
            .collect::<Result<Vec<_>, SqlError>>()?;
        Span::current().record("unusable", unusable.len());
        for (table, index) in &unusable {
            warn!(
                table,
                index, "an index the planner never uses: its build did not finish"
            );
        }
        Ok(unusable)
    }
}

/// Reads every table's columns and primary key in `current_schema()`, the schema unqualified writes land in.
#[instrument(skip(conn), fields(tables = Empty), err(Debug))]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn read_schema(conn: &SqlPool) -> Result<DatabaseSchema, SqlError> {
    let pool = &conn.inner();
    let mut schema = DatabaseSchema::for_database(MigrationDatabase::SqlDb);

    let column_rows = sqlx::query(
        "SELECT table_name, column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_catalog = current_database() \
         ORDER BY table_name, ordinal_position",
    )
    .fetch_all(pool)
    .await?;

    let pk_rows = sqlx::query(
        "SELECT kcu.table_name AS table_name, kcu.column_name AS column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
         WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = current_schema() \
         ORDER BY kcu.table_name, kcu.ordinal_position",
    )
    .fetch_all(pool)
    .await?;

    for row in &column_rows {
        let table: String = row
            .try_get("table_name")
            .map_err(|e| SqlError::Introspection(format!("missing table_name: {e}")))?;
        if is_ledger_table(&table) {
            continue;
        }
        let name: String = row
            .try_get("column_name")
            .map_err(|e| SqlError::Introspection(format!("missing column_name: {e}")))?;
        let raw_type: String = row
            .try_get("data_type")
            .map_err(|e| SqlError::Introspection(format!("missing data_type: {e}")))?;
        let is_nullable: String = row.try_get("is_nullable").unwrap_or_else(|_| "YES".into());
        let default: Option<String> = row.try_get("column_default").ok();

        let data_type = normalize_type(&raw_type, &table, &name);
        let table_schema = schema
            .tables
            .entry(table.clone())
            .or_insert_with(|| TableSchema::new(&table));
        table_schema.add_column(TableColumn {
            name,
            data_type,
            nullable: is_nullable.eq_ignore_ascii_case("YES"),
            default,
            primary_key: false,
            unique: false,
        });
    }

    for row in &pk_rows {
        let table: String = row
            .try_get("table_name")
            .map_err(|e| SqlError::Introspection(format!("missing pk table_name: {e}")))?;
        if is_ledger_table(&table) {
            continue;
        }
        let column: String = row
            .try_get("column_name")
            .map_err(|e| SqlError::Introspection(format!("missing pk column_name: {e}")))?;
        if let Some(table_schema) = schema.tables.get_mut(&table) {
            table_schema.primary_key.push(column.clone());
            if let Some(col) = table_schema.columns.get_mut(&column) {
                col.primary_key = true;
            }
        }
    }

    let tables: Vec<String> = schema.tables.keys().cloned().collect();
    for table in tables {
        let Some(catalog) = super::TableCatalog::read(conn, &table).await? else {
            continue;
        };
        let Some(table_schema) = schema.tables.get_mut(&table) else {
            continue;
        };
        for constraint in catalog
            .constraints
            .iter()
            .filter(|c| c.kind == 'c' && c.table == table)
        {
            let expression = constraint
                .definition
                .strip_prefix("CHECK ")
                .unwrap_or(&constraint.definition)
                .to_owned();
            table_schema.add_check_constraint(TableCheckConstraint {
                name: constraint.name.clone(),
                expression,
            });
        }
        for (role, seen) in &catalog.readers {
            table_schema.add_reader(role.clone(), seen.clone());
        }
        for role in &catalog.writers {
            table_schema.add_writer(role.clone());
        }
        table_schema.colocated = Some(catalog.colocated);
        table_schema.tablets = catalog.tablets;
        for index in &catalog.indexes {
            if index.primary {
                table_schema.set_key(table_key(&table, index)?);
                continue;
            }
            let key = index.key.as_ref().map(key_parts).unwrap_or_default();
            table_schema.add_index(TableIndex {
                name: index.name.clone(),
                columns: key.iter().flat_map(|part| part.columns.clone()).collect(),
                unique: index.unique,
                condition: index.condition.clone(),
                key,
                include: index.include.clone(),
                constraint: catalog
                    .constraints
                    .iter()
                    .any(|c| c.kind == 'u' && c.table == table && c.name == index.name),
            });
        }
    }

    Span::current().record("tables", schema.tables.len());
    debug!(tables = schema.tables.len(), "read sql schema");
    Ok(schema)
}

/// Maps an `information_schema` type name to the one the `Model` derive emits; also a valid cast target.
pub(crate) fn normalize_type(raw: &str, table: &str, column: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    let mapped = match lowered.as_str() {
        "text" | "character varying" | "varchar" | "string" => "TEXT",
        "bigint" | "int8" => "INT8",
        "integer" | "int4" | "int" => "INT4",
        "smallint" | "int2" => "INT2",
        "boolean" | "bool" => "BOOL",
        "uuid" => "UUID",
        "timestamp with time zone" | "timestamptz" => "TIMESTAMPTZ",
        "real" | "float4" => "FLOAT4",
        "double precision" | "float8" => "FLOAT8",
        "bytea" | "bytes" | "blob" => "BYTEA",
        "jsonb" | "json" => "JSONB",
        _ => {
            warn!(
                table,
                column,
                raw_type = raw,
                "un-normalized column type; recording upper-cased, diff may report spurious drift"
            );
            return raw.trim().to_ascii_uppercase();
        }
    };
    mapped.to_string()
}

/// A table's key, read from its primary index; one that does not read is an error, never an empty key.
fn table_key(table: &str, index: &super::CatalogIndex) -> Result<Vec<KeyPart>, SqlError> {
    index.key.as_ref().map(key_parts).ok_or_else(|| {
        SqlError::Introspection(format!(
            "the key of `{table}` does not read as a key list: index {} is `{}`",
            index.name, index.definition
        ))
    })
}

/// A catalog key as table-schema parts; an element with no split is ordered.
fn key_parts(key: &KeyDeclaration) -> Vec<KeyPart> {
    key.elements()
        .iter()
        .map(|element| KeyPart {
            columns: element.columns().to_vec(),
            split: match element.sharding() {
                Some(Sharding::Hash) => ShardKind::Hash,
                Some(Sharding::Desc) => ShardKind::Desc,
                Some(Sharding::Asc) | None => ShardKind::Asc,
            },
            nulls: element.nulls(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary(key: Option<KeyDeclaration>) -> super::super::CatalogIndex {
        super::super::CatalogIndex {
            name: "editions_pkey".to_owned(),
            key,
            definition:
                "CREATE UNIQUE INDEX editions_pkey ON public.editions USING lsm (lower(id))"
                    .to_owned(),
            primary: true,
            unique: true,
            partial: false,
            condition: None,
            expressions: true,
            includes: false,
            include: Vec::new(),
            method: "lsm".to_owned(),
            usable: true,
        }
    }

    #[test]
    fn a_primary_key_that_does_not_read_is_an_error_not_an_empty_key() {
        let error = table_key("editions", &primary(None)).expect_err("an unread key is refused");
        assert!(
            error.to_string().contains("editions_pkey"),
            "the error names the index: {error}"
        );
    }
}
