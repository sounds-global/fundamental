//! Schema diff engine for comparing models to database schema.

mod config;
mod database;
mod schema;
mod table;

pub use config::DiffConfig;
pub use database::DatabaseSchema;
pub use schema::SchemaDiffer;
pub use table::TableSchema;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::{SchemaChange, TableColumn};
    use std::collections::HashSet;

    fn make_column(name: &str, data_type: &str, nullable: bool) -> TableColumn {
        TableColumn {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable,
            default: None,
            primary_key: false,
            unique: false,
        }
    }

    fn keyed(split: &[(&str, fundamental_core::model::sql::ShardKind)]) -> DatabaseSchema {
        let mut schema = DatabaseSchema::new();
        let mut table = TableSchema::new("editions");
        for (column, _) in split {
            table.add_column(make_column(column, "TEXT", false));
        }
        let columns: Vec<_> = split
            .iter()
            .map(|(column, kind)| (*column, Some(*kind)))
            .collect();
        table.set_key(crate::migration::KeyPart::parts(&columns));
        schema.add_table(table);
        schema
    }

    #[test]
    fn a_key_split_otherwise_is_a_changed_key() {
        use fundamental_core::model::sql::ShardKind::{Asc, Desc, Hash};
        let model = keyed(&[
            ("organisation", Hash),
            ("document", Hash),
            ("version", Desc),
        ]);
        let table = keyed(&[("organisation", Hash), ("document", Asc), ("version", Asc)]);
        let changes = SchemaDiffer::new().diff(&model, &table);
        assert!(
            matches!(changes.as_slice(), [SchemaChange::ChangeKey(change)] if change.table_name == "editions"),
            "{changes:?}"
        );
        assert_eq!(
            changes[0].description(),
            "Change the key of 'editions' from (organisation HASH, document ASC, version ASC) to \
             ((organisation, document) HASH, version DESC) (requires table recreation)"
        );
        assert!(SchemaDiffer::new().diff(&model, &model).is_empty());
    }

    #[test]
    fn a_table_with_no_key_is_a_changed_key() {
        use fundamental_core::model::sql::ShardKind::{Asc, Hash};
        let model = keyed(&[("organisation", Hash), ("id", Asc)]);
        let mut table = model.clone();
        table
            .tables
            .get_mut("editions")
            .unwrap()
            .set_key(Vec::new());
        let changes = SchemaDiffer::new().diff(&model, &table);
        assert!(
            matches!(changes.as_slice(), [SchemaChange::ChangeKey(change)] if change.table_name == "editions"),
            "{changes:?}"
        );
        assert_eq!(
            changes[0].description(),
            "Change the key of 'editions' from () to (organisation HASH, id ASC) (requires table \
             recreation)"
        );
    }

    #[test]
    fn a_key_is_compared_only_where_the_reader_reads_keys() {
        use crate::migration::MigrationDatabase::{CqlDb, OfflineDb};
        use fundamental_core::model::sql::ShardKind::Hash;
        let model = keyed(&[("id", Hash)]);
        for database in [OfflineDb, CqlDb] {
            let mut table = model.clone();
            table.database_type = Some(database);
            table
                .tables
                .get_mut("editions")
                .unwrap()
                .set_key(Vec::new());
            assert!(SchemaDiffer::new().diff(&model, &table).is_empty());
        }
    }

    fn placed(colocated: Option<bool>, tablets: Option<u32>, writers: &[&str]) -> DatabaseSchema {
        use fundamental_core::model::sql::ShardKind::Hash;
        let mut schema = keyed(&[("id", Hash)]);
        let table = schema.tables.get_mut("editions").unwrap();
        table.colocated = colocated;
        table.tablets = tablets;
        for role in writers {
            table.add_writer(*role);
        }
        schema
    }

    #[test]
    fn a_writer_the_table_lacks_is_drift_and_one_it_has_besides_is_not() {
        let model = placed(None, None, &["importer"]);

        let lacking = SchemaDiffer::new().diff(&model, &placed(None, None, &[]));
        let besides = SchemaDiffer::new().diff(&placed(None, None, &[]), &model);

        assert!(
            matches!(lacking.as_slice(), [SchemaChange::ChangeWriter(change)]
                if change.role == "importer"),
            "{lacking:?}"
        );
        assert!(besides.is_empty(), "{besides:?}");
    }

    #[test]
    fn a_table_placed_otherwise_or_split_into_fewer_tablets_is_drift_and_more_is_not() {
        let model = placed(Some(false), Some(8), &[]);

        let fewer = SchemaDiffer::new().diff(&model, &placed(Some(true), Some(1), &[]));
        let more = SchemaDiffer::new().diff(&model, &placed(Some(false), Some(16), &[]));
        let unsaid =
            SchemaDiffer::new().diff(&placed(None, None, &[]), &placed(Some(true), Some(1), &[]));

        assert!(
            matches!(fewer.as_slice(), [SchemaChange::ChangePlacement(change)]
                if change.colocated == Some((true, false)) && change.tablets == Some((Some(1), 8))),
            "{fewer:?}"
        );
        assert!(
            fewer[0]
                .description()
                .contains("it has 1 tablets, its model declares 8")
        );
        assert!(more.is_empty(), "{more:?}");
        assert!(unsaid.is_empty(), "{unsaid:?}");
    }

    fn read_by(readers: &[(&str, &str)]) -> DatabaseSchema {
        use fundamental_core::model::sql::ShardKind::Hash;
        let mut schema = keyed(&[("id", Hash)]);
        let table = schema.tables.get_mut("editions").unwrap();
        for (role, when) in readers {
            table.add_reader(*role, *when);
        }
        schema
    }

    #[test]
    fn a_reader_the_table_lacks_or_reads_otherwise_is_a_changed_reader() {
        let model = read_by(&[("www", "visibility = 'published'")]);
        let changes = SchemaDiffer::new().diff(&model, &read_by(&[]));
        assert!(
            matches!(changes.as_slice(), [SchemaChange::ChangeReader(change)]
                if change.table_name == "editions" && change.role == "www"
                    && change.old.is_none()
                    && change.new.as_deref() == Some("visibility = 'published'")),
            "{changes:?}"
        );
        assert_eq!(
            changes[0].description(),
            "Let 'www' read the rows of 'editions' where visibility = 'published' (it reads none)"
        );

        let otherwise = read_by(&[("www", "visibility = 'draft'")]);
        assert_eq!(SchemaDiffer::new().diff(&model, &otherwise).len(), 1);

        let gone = SchemaDiffer::new().diff(&read_by(&[]), &model);
        assert!(
            matches!(gone.as_slice(), [SchemaChange::ChangeReader(change)]
                if change.new.is_none() && change.old.is_some()),
            "{gone:?}"
        );
        assert_eq!(
            gone[0].description(),
            "Let 'www' read no rows of 'editions' (it reads where visibility = 'published')"
        );
    }

    #[test]
    fn a_reader_is_the_same_however_the_engine_renders_its_rows() {
        let model = read_by(&[("www", "visibility = 'published' AND deleted_at IS NULL")]);
        let table = read_by(&[(
            "www",
            "((visibility = 'published'::text) AND (deleted_at IS NULL))",
        )]);
        assert!(SchemaDiffer::new().diff(&model, &table).is_empty());
    }

    #[test]
    fn test_diff_add_table() {
        let mut expected = DatabaseSchema::new();
        let mut table = TableSchema::new("users");
        table.add_column(make_column("id", "UUID", false));
        table.add_column(make_column("name", "TEXT", false));
        expected.add_table(table);

        let actual = DatabaseSchema::new();

        let differ = SchemaDiffer::new();
        let changes = differ.diff(&expected, &actual);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SchemaChange::CreateTable(_)));
    }

    #[test]
    fn test_diff_drop_table() {
        let expected = DatabaseSchema::new();

        let mut actual = DatabaseSchema::new();
        let mut table = TableSchema::new("old_table");
        table.add_column(make_column("id", "UUID", false));
        actual.add_table(table);

        let differ = SchemaDiffer::new();
        let changes = differ.diff(&expected, &actual);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SchemaChange::DropTable(_)));
    }

    #[test]
    fn test_diff_add_column() {
        let mut expected = DatabaseSchema::new();
        let mut exp_table = TableSchema::new("users");
        exp_table.add_column(make_column("id", "UUID", false));
        exp_table.add_column(make_column("name", "TEXT", false));
        exp_table.add_column(make_column("email", "TEXT", true)); // New column
        expected.add_table(exp_table);

        let mut actual = DatabaseSchema::new();
        let mut act_table = TableSchema::new("users");
        act_table.add_column(make_column("id", "UUID", false));
        act_table.add_column(make_column("name", "TEXT", false));
        actual.add_table(act_table);

        let differ = SchemaDiffer::new();
        let changes = differ.diff(&expected, &actual);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SchemaChange::AddColumn(_)));
    }

    #[test]
    fn test_diff_drop_column() {
        let mut expected = DatabaseSchema::new();
        let mut exp_table = TableSchema::new("users");
        exp_table.add_column(make_column("id", "UUID", false));
        expected.add_table(exp_table);

        let mut actual = DatabaseSchema::new();
        let mut act_table = TableSchema::new("users");
        act_table.add_column(make_column("id", "UUID", false));
        act_table.add_column(make_column("old_column", "TEXT", true));
        actual.add_table(act_table);

        let differ = SchemaDiffer::new();
        let changes = differ.diff(&expected, &actual);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SchemaChange::DropColumn(_)));
    }

    #[test]
    fn test_diff_alter_column() {
        let mut expected = DatabaseSchema::new();
        let mut exp_table = TableSchema::new("users");
        exp_table.add_column(make_column("id", "UUID", false));
        exp_table.add_column(make_column("name", "TEXT", false)); // Not nullable
        expected.add_table(exp_table);

        let mut actual = DatabaseSchema::new();
        let mut act_table = TableSchema::new("users");
        act_table.add_column(make_column("id", "UUID", false));
        act_table.add_column(make_column("name", "TEXT", true)); // Nullable
        actual.add_table(act_table);

        let differ = SchemaDiffer::new();
        let changes = differ.diff(&expected, &actual);

        assert_eq!(changes.len(), 1);
        assert!(matches!(changes[0], SchemaChange::AlterColumn(_)));
    }

    #[test]
    fn test_type_normalization() {
        let differ = SchemaDiffer::new();
        assert_eq!(
            differ.normalize_type("INT"),
            differ.normalize_type("INTEGER")
        );
        assert_eq!(
            differ.normalize_type("INT4"),
            differ.normalize_type("INTEGER")
        );
        assert_eq!(
            differ.normalize_type("BOOL"),
            differ.normalize_type("BOOLEAN")
        );
        assert_eq!(
            differ.normalize_type("VARCHAR(255)"),
            differ.normalize_type("TEXT")
        );
    }

    fn schema_with(table: &str, cols: &[(&str, &str, bool)]) -> DatabaseSchema {
        let mut db = DatabaseSchema::new();
        let mut t = TableSchema::new(table);
        for (name, ty, nullable) in cols {
            t.add_column(make_column(name, ty, *nullable));
        }
        db.add_table(t);
        db
    }

    fn renames(changes: &[SchemaChange]) -> Vec<(&str, &str)> {
        changes
            .iter()
            .filter_map(|c| match c {
                SchemaChange::RenameColumn(r) => Some((r.old_name.as_str(), r.new_name.as_str())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn detect_renames_matches_same_signature_as_rename_not_add_drop() {
        let differ = SchemaDiffer::with_config(DiffConfig {
            detect_renames: true,
            ..Default::default()
        });
        let expected = schema_with("t", &[("id", "UUID", false), ("full_name", "TEXT", false)]);
        let actual = schema_with("t", &[("id", "UUID", false), ("name", "TEXT", false)]);

        let changes = differ.diff(&expected, &actual);

        assert_eq!(renames(&changes), vec![("name", "full_name")]);
        assert!(
            !changes
                .iter()
                .any(|c| matches!(c, SchemaChange::AddColumn(_) | SchemaChange::DropColumn(_))),
            "a matched rename must not also emit add/drop: {changes:?}"
        );
    }

    #[test]
    fn detect_renames_pairs_each_dropped_with_a_distinct_added_column() {
        // Two drops and two adds share one signature: each must pair with a distinct add.
        let differ = SchemaDiffer::with_config(DiffConfig {
            detect_renames: true,
            ..Default::default()
        });
        let expected = schema_with("t", &[("a2", "TEXT", false), ("b2", "TEXT", false)]);
        let actual = schema_with("t", &[("a1", "TEXT", false), ("b1", "TEXT", false)]);

        let changes = differ.diff(&expected, &actual);
        let mut got = renames(&changes);
        got.sort_unstable();

        assert_eq!(got.len(), 2, "expected two renames: {changes:?}");
        let new_names: HashSet<&str> = got.iter().map(|(_, new)| *new).collect();
        assert_eq!(
            new_names.len(),
            2,
            "each added column matched once: {got:?}"
        );
        assert!(
            !changes
                .iter()
                .any(|c| matches!(c, SchemaChange::AddColumn(_) | SchemaChange::DropColumn(_)))
        );
    }

    #[test]
    fn detect_renames_does_not_match_across_differing_signatures() {
        let differ = SchemaDiffer::with_config(DiffConfig {
            detect_renames: true,
            ..Default::default()
        });
        let expected = schema_with("t", &[("full_name", "TEXT", true)]); // nullable differs
        let actual = schema_with("t", &[("name", "TEXT", false)]);

        let changes = differ.diff(&expected, &actual);

        assert!(renames(&changes).is_empty(), "must not rename: {changes:?}");
        assert!(
            changes
                .iter()
                .any(|c| matches!(c, SchemaChange::AddColumn(_)))
        );
        assert!(
            changes
                .iter()
                .any(|c| matches!(c, SchemaChange::DropColumn(_)))
        );
    }
}
