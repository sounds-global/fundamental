//! The per-table side of the diff model.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::migration::{
    KeyPart, MigrationDatabase, Retired, TableCheckConstraint, TableColumn, TableForeignKey,
    TableIndex,
};

use fundamental_core::model::cql::{ClusteringOrder, TableOptions};
use fundamental_core::model::sql::Lint;

/// Represents a table schema.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Table name.
    pub name: String,
    /// Columns.
    pub columns: HashMap<String, TableColumn>,
    /// Column order (for deterministic output).
    pub column_order: Vec<String>,
    /// Primary key columns.
    pub primary_key: Vec<String>,
    /// The primary key's parts, each with how it is split.
    pub key: Vec<KeyPart>,
    /// Indexes.
    pub indexes: HashMap<String, TableIndex>,
    /// Foreign keys.
    pub foreign_keys: HashMap<String, TableForeignKey>,
    /// Check constraints.
    pub check_constraints: HashMap<String, TableCheckConstraint>,
    /// YCQL: partition key columns.
    pub partition_key: Option<Vec<String>>,
    /// YCQL: clustering key.
    pub clustering_key: Option<Vec<(String, ClusteringOrder)>>,
    /// YCQL: table options.
    pub table_options: Option<TableOptions>,
    /// The columns the model renamed: each one's name, to the name its migrations knew it by.
    pub renamed: HashMap<String, String>,
    /// What the model retired, for the next migration to drop.
    pub retired: Retired,
    /// The roles that may read the table, each with the rows it sees, as SQL; a reader writes
    /// nothing.
    pub readers: BTreeMap<String, String>,
    /// The roles that may write the table: each reads, adds, changes and deletes any row.
    pub writers: BTreeSet<String>,
    /// Whether the table shares the colocated database's one tablet (`Some(true)`), has its own
    /// (`Some(false)`), or is as the database places it (`None`).
    pub colocated: Option<bool>,
    /// The number of tablets the table is split into when it is created; none leaves it to the
    /// engine.
    pub tablets: Option<u32>,
    /// The lints the model accepts on this table, each with its reason.
    pub accepted: BTreeMap<Lint, String>,
}

impl TableSchema {
    /// Creates a new table schema.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: HashMap::new(),
            column_order: Vec::new(),
            primary_key: Vec::new(),
            key: Vec::new(),
            indexes: HashMap::new(),
            foreign_keys: HashMap::new(),
            check_constraints: HashMap::new(),
            partition_key: None,
            clustering_key: None,
            table_options: None,
            renamed: HashMap::new(),
            retired: Retired::default(),
            readers: BTreeMap::new(),
            writers: BTreeSet::new(),
            colocated: None,
            tablets: None,
            accepted: BTreeMap::new(),
        }
    }

    /// Lets the role `role` read, add, change and delete any row of the table.
    pub fn add_writer(&mut self, role: impl Into<String>) {
        self.writers.insert(role.into());
    }

    /// Says whether the table shares the colocated database's tablet.
    pub fn set_colocated(&mut self, colocated: bool) {
        self.colocated = Some(colocated);
    }

    /// Splits the table into `tablets` tablets when it is created.
    pub fn set_tablets(&mut self, tablets: u32) {
        self.tablets = Some(tablets);
    }

    /// Accepts `lint` on this table, `because` of a stated reason.
    pub fn accept(&mut self, lint: Lint, because: impl Into<String>) {
        self.accepted.insert(lint, because.into());
    }

    /// Lets the role `role` read the rows `when` holds true for, as SQL, and nothing else.
    pub fn add_reader(&mut self, role: impl Into<String>, when: impl Into<String>) {
        self.readers.insert(role.into(), when.into());
    }

    /// Adds a column.
    pub fn add_column(&mut self, column: TableColumn) {
        let name = column.name.clone();
        self.columns.insert(name.clone(), column);
        if !self.column_order.contains(&name) {
            self.column_order.push(name);
        }
    }

    /// Adds an index.
    pub fn add_index(&mut self, index: TableIndex) {
        self.indexes.insert(index.name.clone(), index);
    }

    /// Adds a foreign key.
    pub fn add_foreign_key(&mut self, fk: TableForeignKey) {
        self.foreign_keys.insert(fk.name.clone(), fk);
    }

    /// Adds a check constraint.
    pub fn add_check_constraint(&mut self, constraint: TableCheckConstraint) {
        self.check_constraints
            .insert(constraint.name.clone(), constraint);
    }

    /// Sets the primary key.
    pub fn set_primary_key(&mut self, columns: Vec<String>) {
        self.primary_key = columns;
    }

    /// Sets the primary key's parts, each with how it is split.
    pub fn set_key(&mut self, key: Vec<KeyPart>) {
        self.key = key;
    }

    /// Says the column `name` was called `was` before, so the next migration renames it.
    pub fn rename_column(&mut self, name: impl Into<String>, was: impl Into<String>) {
        self.renamed.insert(name.into(), was.into());
    }

    /// Retires the column `name`: the next migration drops it.
    pub fn retire_column(&mut self, name: impl Into<String>) {
        self.retired.columns.push(name.into());
    }

    /// Retires the index `name`: the next migration drops it.
    pub fn retire_index(&mut self, name: impl Into<String>) {
        self.retired.indexes.push(name.into());
    }

    /// Retires the check `name`: the next migration drops it.
    pub fn retire_check(&mut self, name: impl Into<String>) {
        self.retired.checks.push(name.into());
    }

    /// Sets YCQL partition key.
    pub fn set_partition_key(&mut self, columns: Vec<String>) {
        self.partition_key = Some(columns);
    }

    /// Sets YCQL clustering key.
    pub fn set_clustering_key(&mut self, columns: Vec<(String, ClusteringOrder)>) {
        self.clustering_key = Some(columns);
    }

    /// The companion table that holds this table's text in its other languages: `<name>_i18n`,
    /// keyed by this table's own key columns, then the field and the language, typed for
    /// `database`, so a record's translations sit beside it.
    #[must_use]
    pub fn translations(&self, database: MigrationDatabase) -> Self {
        let mut companion = Self::new(format!("{}_i18n", self.name));
        self.translation_columns(&mut companion, database);
        let mut primary_key = self.primary_key.clone();
        primary_key.extend(["field".to_owned(), "locale".to_owned()]);
        companion.set_primary_key(primary_key);
        let mut key = self.key.clone();
        for column in ["field", "locale"] {
            key.push(KeyPart {
                columns: vec![column.to_owned()],
                split: fundamental_core::model::sql::ShardKind::Asc,
                nulls: None,
            });
        }
        companion.set_key(key);
        if database == MigrationDatabase::CqlDb {
            companion.partition_key.clone_from(&self.partition_key);
            let mut clustering = self.clustering_key.clone().unwrap_or_default();
            for column in ["field", "locale"] {
                clustering.push((column.to_owned(), ClusteringOrder::Asc));
            }
            companion.set_clustering_key(clustering);
        }
        self.read_with(&mut companion);
        companion
    }

    /// The columns of [`translations`](Self::translations), typed for `database`.
    fn translation_columns(&self, companion: &mut Self, database: MigrationDatabase) {
        // YCQL has neither nullability nor defaults: every column reads back as nullable.
        let cql = database == MigrationDatabase::CqlDb;
        let (text_type, at_type) = match database {
            MigrationDatabase::SqlDb => ("TEXT", "TIMESTAMPTZ"),
            MigrationDatabase::CqlDb => ("text", "bigint"),
            MigrationDatabase::OfflineDb => ("TEXT", "INTEGER"),
        };
        for key in &self.primary_key {
            if let Some(column) = self.columns.get(key) {
                companion.add_column(TableColumn {
                    nullable: cql,
                    default: None,
                    primary_key: true,
                    unique: false,
                    ..column.clone()
                });
            }
        }
        let column = |name: &str,
                      data_type: &str,
                      primary_key: bool,
                      nullable: bool,
                      default: Option<&str>| TableColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: nullable || cql,
            default: default.filter(|_| !cql).map(str::to_owned),
            primary_key,
            unique: false,
        };
        companion.add_column(column("field", text_type, true, false, None));
        companion.add_column(column("locale", text_type, true, false, None));
        companion.add_column(column("value", text_type, false, false, None));
        companion.add_column(column("source_hash", text_type, false, false, Some("''")));
        companion.add_column(column(
            "translation_hash",
            text_type,
            false,
            false,
            Some("''"),
        ));
        companion.add_column(column("checked_by", text_type, false, true, None));
        companion.add_column(column("checked_at", at_type, false, true, None));
    }

    /// `companion` read by each role that reads this table, limited to the records it reads.
    fn read_with(&self, companion: &mut Self) {
        let parent = &self.name;
        let matched: Vec<String> = self
            .primary_key
            .iter()
            .map(|column| format!("{parent}.{column} = {}.{column}", companion.name))
            .collect();
        for role in self.readers.keys() {
            companion.add_reader(
                role.clone(),
                format!(
                    "EXISTS (SELECT 1 FROM {parent} WHERE {})",
                    matched.join(" AND ")
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::model::sql::ShardKind;

    fn column(name: &str, data_type: &str, primary_key: bool) -> TableColumn {
        TableColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: false,
            default: None,
            primary_key,
            unique: false,
        }
    }

    /// A record of one organisation, keyed `(organisation_id HASH, id ASC)`, with a title.
    fn articles() -> TableSchema {
        let mut table = TableSchema::new("articles");
        table.add_column(column("organisation_id", "UUID", true));
        table.add_column(column("id", "UUID", true));
        table.add_column(column("title", "TEXT", false));
        table.set_primary_key(vec!["organisation_id".to_owned(), "id".to_owned()]);
        table.set_key(KeyPart::parts(&[
            ("organisation_id", Some(ShardKind::Hash)),
            ("id", Some(ShardKind::Asc)),
        ]));
        table
    }

    #[test]
    fn a_companion_carries_its_records_key_first_then_the_field_and_the_language() {
        let companion = articles().translations(MigrationDatabase::SqlDb);

        assert_eq!(companion.name, "articles_i18n");
        assert_eq!(
            companion.column_order,
            [
                "organisation_id",
                "id",
                "field",
                "locale",
                "value",
                "source_hash",
                "translation_hash",
                "checked_by",
                "checked_at"
            ]
        );
        assert_eq!(companion.columns["organisation_id"].data_type, "UUID");
        assert_eq!(companion.columns["id"].data_type, "UUID");
        assert!(
            companion
                .columns
                .values()
                .all(|column| column.nullable == column.name.starts_with("checked_")),
            "who confirmed a text still fits, and when, is there only once someone did"
        );
        assert_eq!(companion.columns["checked_at"].data_type, "TIMESTAMPTZ");
        for fingerprint in ["source_hash", "translation_hash"] {
            assert_eq!(
                companion.columns[fingerprint].default.as_deref(),
                Some("''")
            );
        }
        assert_eq!(
            companion.primary_key,
            ["organisation_id", "id", "field", "locale"]
        );
        assert_eq!(
            companion.key,
            KeyPart::parts(&[
                ("organisation_id", Some(ShardKind::Hash)),
                ("id", Some(ShardKind::Asc)),
                ("field", Some(ShardKind::Asc)),
                ("locale", Some(ShardKind::Asc)),
            ])
        );
    }

    /// A YCQL record of one organisation: partitioned by `organisation_id`, clustered by `id`
    /// newest first.
    fn cql_notices() -> TableSchema {
        let mut table = TableSchema::new("notices");
        let cql = |name: &str, data_type: &str, primary_key: bool| TableColumn {
            nullable: true,
            ..column(name, data_type, primary_key)
        };
        table.add_column(cql("organisation_id", "uuid", true));
        table.add_column(cql("id", "bigint", true));
        table.add_column(cql("title", "text", false));
        table.set_primary_key(vec!["organisation_id".to_owned(), "id".to_owned()]);
        table.set_partition_key(vec!["organisation_id".to_owned()]);
        table.set_clustering_key(vec![("id".to_owned(), ClusteringOrder::Desc)]);
        table
    }

    #[test]
    fn a_cql_companion_shares_its_records_partition_and_clusters_by_its_key() {
        let companion = cql_notices().translations(MigrationDatabase::CqlDb);

        assert_eq!(companion.name, "notices_i18n");
        assert_eq!(
            companion.partition_key.as_deref(),
            Some(&["organisation_id".to_owned()][..])
        );
        assert_eq!(
            companion.clustering_key.as_deref(),
            Some(
                &[
                    ("id".to_owned(), ClusteringOrder::Desc),
                    ("field".to_owned(), ClusteringOrder::Asc),
                    ("locale".to_owned(), ClusteringOrder::Asc),
                ][..]
            )
        );
        assert_eq!(
            companion.primary_key,
            ["organisation_id", "id", "field", "locale"]
        );
        assert_eq!(companion.columns["organisation_id"].data_type, "uuid");
        assert_eq!(companion.columns["id"].data_type, "bigint");
        assert_eq!(companion.columns["value"].data_type, "text");
        assert_eq!(companion.columns["checked_at"].data_type, "bigint");
        assert!(
            companion
                .columns
                .values()
                .all(|column| column.nullable && column.default.is_none()),
            "YCQL has no nullability and no defaults, and reads every column back as nullable"
        );
    }

    #[test]
    fn an_offline_companion_carries_its_records_key_column_with_its_sqlite_type() {
        let mut table = TableSchema::new("drafts");
        table.add_column(column("id", "INTEGER", true));
        table.add_column(column("title", "TEXT", false));
        table.set_primary_key(vec!["id".to_owned()]);

        let companion = table.translations(MigrationDatabase::OfflineDb);

        assert_eq!(companion.name, "drafts_i18n");
        assert_eq!(companion.primary_key, ["id", "field", "locale"]);
        assert_eq!(companion.columns["id"].data_type, "INTEGER");
        assert_eq!(companion.columns["value"].data_type, "TEXT");
        assert_eq!(companion.columns["checked_at"].data_type, "INTEGER");
        assert_eq!(
            companion.columns["source_hash"].default.as_deref(),
            Some("''")
        );
        assert!(companion.partition_key.is_none());
    }
}
