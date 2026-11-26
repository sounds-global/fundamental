//! [`SQL_TUPLE_MIGRATIONS`]: the relation-tuple table, as migrations an app's migrator applies.

use fundamental_db::migration::EmbeddedMigrations;

use super::relation_tuple_row::RelationTupleRow;

/// The relation-tuple table and its two read-path indexes, for every database a
/// [`SqlTupleStore`](crate::backend::SqlTupleStore) serves.
///
/// Register it with `MigrateRunner::with_embedded(&SQL_TUPLE_MIGRATIONS)` or `migrate_main!`'s
/// `embedded` list; the serving role then needs only `SELECT, INSERT, UPDATE, DELETE`.
pub static SQL_TUPLE_MIGRATIONS: EmbeddedMigrations =
    fundamental_macros::migrations!(owner = "fundamental_authz", models = [RelationTupleRow]);

#[cfg(test)]
mod tests {
    use fundamental_db::migration::ddl::{DdlStatement, recognise};
    use fundamental_db::migration::{Lints, MigrationGuard, split_statements};

    use super::{RelationTupleRow, SQL_TUPLE_MIGRATIONS};

    fn statements() -> Vec<DdlStatement> {
        SQL_TUPLE_MIGRATIONS
            .migrations()
            .iter()
            .flat_map(|migration| split_statements(migration.up()))
            .map(|statement| recognise(&statement).expect("the shipped DDL reads"))
            .collect()
    }

    #[test]
    fn the_table_is_the_one_the_model_reads() {
        let shipped = statements()
            .into_iter()
            .find_map(|statement| match statement {
                DdlStatement::CreateTable {
                    table, primary_key, ..
                } => Some((table, primary_key)),
                _ => None,
            })
            .expect("the set creates a table");
        let DdlStatement::CreateTable {
            table, primary_key, ..
        } = recognise(RelationTupleRow::create_table_sql()).expect("the model's DDL reads")
        else {
            panic!("the model's DDL is a CREATE TABLE");
        };
        assert_eq!(shipped, (table, primary_key));
    }

    #[test]
    fn the_set_creates_both_read_path_indexes() {
        let indexes: Vec<String> = statements()
            .into_iter()
            .filter_map(|statement| match statement {
                DdlStatement::CreateIndex { name, table, .. } => {
                    assert_eq!(table, "authz_relation_tuple");
                    Some(name)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            indexes,
            [
                "authz_relation_tuple_forward_idx",
                "authz_relation_tuple_reverse_idx"
            ]
        );
    }

    #[test]
    fn a_production_guard_accepts_every_key() {
        let migrations: Vec<(String, &str)> = SQL_TUPLE_MIGRATIONS
            .migrations()
            .iter()
            .map(|migration| (SQL_TUPLE_MIGRATIONS.ledger_name(migration), migration.up()))
            .collect();
        let named: Vec<(&str, &str)> = migrations
            .iter()
            .map(|(name, sql)| (name.as_str(), *sql))
            .collect();
        MigrationGuard::new(1_000, Lints::default(), &[])
            .inspect_sql(&named)
            .expect("every key element declares how it shards");
    }

    #[test]
    fn the_set_is_recorded_under_its_crate() {
        assert_eq!(
            SQL_TUPLE_MIGRATIONS.ledger_name(&SQL_TUPLE_MIGRATIONS.migrations()[0]),
            "fundamental_authz/0001_authz_relation_tuple"
        );
    }
}
