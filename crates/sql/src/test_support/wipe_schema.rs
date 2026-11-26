use std::collections::{HashMap, HashSet};

use sqlx::Row;

use super::{is_inlinable, schema_exists};
use fundamental_db::migration::is_ledger_table;

use crate::{SqlError, SqlPool, SqlResult};

/// Empties every application table in `schema`, children before parents, leaving the schema in place; returns how many.
///
/// Run on acquire, so a killed holder cannot leak rows. `DELETE`, not `TRUNCATE`, which takes
/// the cluster-wide catalog latch.
///
/// # Errors
/// Returns [`SqlError::SchemaMissing`](crate::SqlError::SchemaMissing) if the catalog does
/// not have `schema`, and the underlying [`SqlError`](crate::SqlError) if the listing or a
/// `DELETE` fails.
pub async fn wipe_schema(pool: &SqlPool, schema: &str) -> SqlResult<usize> {
    if !is_inlinable(schema) {
        eprintln!("wipe: refusing to inline '{schema}'; nothing was emptied");
        return Ok(0);
    }
    if !schema_exists(pool, schema).await? {
        return Err(SqlError::SchemaMissing {
            schema: schema.to_owned(),
        });
    }

    let tables = tables_in(pool, schema).await?;
    if tables.is_empty() {
        return Ok(0);
    }
    let ordered = children_first(tables, foreign_keys_in(pool, schema).await?);

    let mut emptied = 0;
    for table in &ordered {
        pool.execute(&format!("DELETE FROM {schema}.{table}"))
            .await?;
        emptied += 1;
    }
    Ok(emptied)
}

async fn tables_in(pool: &SqlPool, schema: &str) -> SqlResult<Vec<String>> {
    let rows = pool
        .fetch_all(&format!(
            "SELECT tablename::text FROM pg_tables \
              WHERE schemaname = '{schema}' ORDER BY tablename"
        ))
        .await?;

    let mut tables = Vec::with_capacity(rows.len());
    for row in &rows {
        let name: String = row.try_get(0)?;
        // The migrator's ledger describes the schema; emptying it would make the next run rebuild it.
        if is_ledger_table(&name) {
            continue;
        }
        if is_inlinable(&name) {
            tables.push(name);
        } else {
            eprintln!("wipe: refusing to inline '{schema}.{name}'; it was left alone");
        }
    }
    Ok(tables)
}

/// Every `(child, parent)` foreign-key edge inside `schema`.
async fn foreign_keys_in(pool: &SqlPool, schema: &str) -> SqlResult<Vec<(String, String)>> {
    let rows = pool
        .fetch_all(&format!(
            "SELECT child.relname::text, parent.relname::text \
               FROM pg_constraint constraints \
               JOIN pg_class child ON child.oid = constraints.conrelid \
               JOIN pg_class parent ON parent.oid = constraints.confrelid \
               JOIN pg_namespace child_schema ON child_schema.oid = child.relnamespace \
               JOIN pg_namespace parent_schema ON parent_schema.oid = parent.relnamespace \
              WHERE constraints.contype = 'f' \
                AND child_schema.nspname = '{schema}' \
                AND parent_schema.nspname = '{schema}'"
        ))
        .await?;

    let mut edges = Vec::with_capacity(rows.len());
    for row in &rows {
        let child: String = row.try_get(0)?;
        let parent: String = row.try_get(1)?;
        if child != parent {
            edges.push((child, parent));
        }
    }
    Ok(edges)
}

/// Orders `tables` children first; the visited set breaks cycles.
fn children_first(tables: Vec<String>, edges: Vec<(String, String)>) -> Vec<String> {
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (child, parent) in &edges {
        children.entry(parent.as_str()).or_default().push(child);
    }

    let mut ordered: Vec<String> = Vec::with_capacity(tables.len());
    let mut placed: HashSet<&str> = HashSet::new();
    for table in &tables {
        visit(table.as_str(), &children, &mut placed, &mut ordered);
    }
    ordered
}

fn visit<'a>(
    table: &'a str,
    children: &HashMap<&'a str, Vec<&'a str>>,
    placed: &mut HashSet<&'a str>,
    ordered: &mut Vec<String>,
) {
    if !placed.insert(table) {
        return;
    }
    for child in children.get(table).into_iter().flatten() {
        visit(child, children, placed, ordered);
    }
    ordered.push(table.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ledger_tables_are_never_in_the_wipe() {
        for table in [
            "_fundamental_migrations",
            "_fundamental_migration_steps",
            "_fundamental_migration_resolutions",
        ] {
            assert!(is_ledger_table(table), "{table}");
        }
    }

    #[test]
    fn a_child_is_emptied_before_its_parent() {
        let ordered = children_first(
            vec!["credentials".to_owned(), "issuers".to_owned()],
            vec![("credentials".to_owned(), "issuers".to_owned())],
        );
        let child = ordered.iter().position(|t| t == "credentials");
        let parent = ordered.iter().position(|t| t == "issuers");
        assert!(child < parent, "{ordered:?} would violate the foreign key");
    }

    #[test]
    fn a_chain_is_emptied_leaf_first() {
        let ordered = children_first(
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            vec![
                ("c".to_owned(), "b".to_owned()),
                ("b".to_owned(), "a".to_owned()),
            ],
        );
        assert_eq!(
            ordered,
            vec!["c".to_owned(), "b".to_owned(), "a".to_owned()]
        );
    }

    #[test]
    fn a_cycle_is_ordered_rather_than_looping_forever() {
        let ordered = children_first(
            vec!["left".to_owned(), "right".to_owned()],
            vec![
                ("left".to_owned(), "right".to_owned()),
                ("right".to_owned(), "left".to_owned()),
            ],
        );
        assert_eq!(ordered.len(), 2);
    }

    #[test]
    fn every_table_is_emptied_exactly_once() {
        let ordered = children_first(
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            vec![
                ("a".to_owned(), "c".to_owned()),
                ("b".to_owned(), "c".to_owned()),
            ],
        );
        assert_eq!(ordered.len(), 3);
        let unique: HashSet<&String> = ordered.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn a_schema_name_that_could_end_a_literal_is_refused() {
        assert!(!is_inlinable("s'; DROP SCHEMA prod; --"));
    }
}
