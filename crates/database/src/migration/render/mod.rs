//! A table schema as the YSQL a person writes for it, every statement guarded so it can run
//! again; YCQL and SQLite tables by [`create_cql_table`] and [`create_sqlite_table`].

mod cql;
mod sqlite;

pub use cql::create_cql_table;
pub use sqlite::create_sqlite_table;

use crate::migration::ddl::{DdlStatement, recognise};
use crate::migration::{KeyPart, TableIndex, TableSchema};

/// `CREATE TABLE` for `table`: its columns, checks and key, each key part split as declared.
#[must_use]
pub fn create_table(table: &TableSchema) -> String {
    let mut lines: Vec<String> = table
        .column_order
        .iter()
        .filter_map(|name| table.columns.get(name))
        .map(|column| {
            let mut line = format!("    {} {}", column.name, column.data_type);
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = &column.default {
                line.push_str(" DEFAULT ");
                line.push_str(default);
            }
            line
        })
        .collect();
    let mut checks: Vec<_> = table.check_constraints.values().collect();
    checks.sort_by(|left, right| left.name.cmp(&right.name));
    for check in checks {
        lines.push(format!(
            "    CONSTRAINT {} CHECK ({})",
            check.name, check.expression
        ));
    }
    if !table.key.is_empty() {
        lines.push(format!("    PRIMARY KEY ({})", key(&table.key)));
    }
    let mut placed = String::new();
    if table.colocated == Some(false) {
        placed.push_str(" WITH (COLOCATION = false)");
    }
    if let Some(tablets) = table.tablets {
        placed.push_str(&format!(" SPLIT INTO {tablets} TABLETS"));
    }
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n){placed};",
        table.name,
        lines.join(",\n")
    )
}

/// `CREATE INDEX` for `index` on `table`, each key part split as declared.
#[must_use]
pub fn create_index(table: &str, index: &TableIndex) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let mut statement = format!(
        "CREATE {unique}INDEX IF NOT EXISTS {}\n    ON {table} ({})",
        index.name,
        key(&index.key)
    );
    if !index.include.is_empty() {
        statement.push_str(&format!(" INCLUDE ({})", index.include.join(", ")));
    }
    if let Some(condition) = &index.condition {
        statement.push_str(&format!(" WHERE {condition}"));
    }
    statement.push(';');
    statement
}

/// The statements that create `table` and its indexes, then grant its readers and writers.
#[must_use]
pub fn create_all(table: &TableSchema) -> Vec<String> {
    let mut indexes: Vec<&TableIndex> = table.indexes.values().collect();
    indexes.sort_by(|left, right| left.name.cmp(&right.name));
    std::iter::once(create_table(table))
        .chain(
            indexes
                .into_iter()
                .map(|index| create_index(&table.name, index)),
        )
        .chain(guard_rows(&table.name, !table.readers.is_empty()))
        .chain(
            table
                .readers
                .iter()
                .flat_map(|(role, when)| add_reader(&table.name, role, when)),
        )
        .chain(
            table
                .writers
                .iter()
                .flat_map(|role| add_writer(&table.name, role)),
        )
        .collect()
}

/// Row security on `table`, when it has `readers`; every other role still sees every row.
pub(crate) fn guard_rows(table: &str, readers: bool) -> Vec<String> {
    if !readers {
        return Vec::new();
    }
    vec![
        format!("ALTER TABLE {table} ENABLE ROW LEVEL SECURITY;"),
        once(&format!(
            "CREATE POLICY {table}_written ON {table} USING (true) WITH CHECK (true)"
        )),
    ]
}

/// Row security off `table` once its last reader is gone.
pub(crate) fn unguard_rows(table: &str) -> Vec<String> {
    vec![
        format!("DROP POLICY IF EXISTS {table}_written ON {table};"),
        format!("ALTER TABLE {table} DISABLE ROW LEVEL SECURITY;"),
    ]
}

/// The role `role`, made when absent, allowed to read only the rows of `table` that `when`
/// holds for, through a restrictive policy and a `SELECT` grant.
///
/// The role is looked up before it is made, so a migrator without `CREATEROLE` can run this
/// once an administrator has made the role.
pub(crate) fn add_reader(table: &str, role: &str, when: &str) -> Vec<String> {
    vec![
        make_role(role),
        once(&format!(
            "CREATE POLICY {} ON {table} AS RESTRICTIVE FOR SELECT TO {role} USING ({when})",
            reader_policy(table, role)
        )),
        format!("GRANT SELECT ON {table} TO {role};"),
    ]
}

/// The role `role`, made when absent, allowed to read, add, change and delete every row of
/// `table`.
pub(crate) fn add_writer(table: &str, role: &str) -> Vec<String> {
    vec![
        make_role(role),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON {table} TO {role};"),
    ]
}

/// The role `role` no longer let write `table`.
pub(crate) fn drop_writer(table: &str, role: &str) -> Vec<String> {
    vec![format!(
        "REVOKE SELECT, INSERT, UPDATE, DELETE ON {table} FROM {role};"
    )]
}

fn make_role(role: &str) -> String {
    once(&format!(
        "IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') THEN \
         CREATE ROLE {role} NOLOGIN; END IF"
    ))
}

/// `statement` as the generator writes it now, when an older generator wrote it another way;
/// `None` otherwise.
pub(crate) fn regenerated(statement: &str) -> Option<String> {
    match recognise(statement) {
        Ok(DdlStatement::CreateRole {
            role,
            looked_up: false,
        }) if statement == once(&format!("CREATE ROLE {role} NOLOGIN")) => Some(make_role(&role)),
        _ => None,
    }
}

/// The role `role` no longer let read `table`.
pub(crate) fn drop_reader(table: &str, role: &str) -> Vec<String> {
    vec![
        format!("REVOKE SELECT ON {table} FROM {role};"),
        format!(
            "DROP POLICY IF EXISTS {} ON {table};",
            reader_policy(table, role)
        ),
    ]
}

pub(crate) fn reader_policy(table: &str, role: &str) -> String {
    format!("{table}_read_by_{role}")
}

/// `statement`, which has no `IF NOT EXISTS`, made so it can run again.
fn once(statement: &str) -> String {
    format!("DO $$ BEGIN {statement}; EXCEPTION WHEN duplicate_object THEN NULL; END $$;")
}

/// A key's parts as YSQL writes them: `(a, b) HASH, c ASC`, `at DESC NULLS LAST`.
pub(crate) fn key(parts: &[KeyPart]) -> String {
    parts
        .iter()
        .map(|part| {
            let columns = if part.columns.len() > 1 {
                format!("({})", part.columns.join(", "))
            } else {
                part.columns.join(", ")
            };
            let mut written = format!("{columns} {}", part.split.as_sql());
            if let Some(nulls) = part.nulls {
                written.push(' ');
                written.push_str(nulls.as_sql());
            }
            written
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::sql::ShardKind;

    use super::{create_all, create_index};
    use crate::migration::{
        KeyPart, Lints, MigrationGuard, Nulls, TableColumn, TableIndex, TableSchema,
    };

    fn column(name: &str, data_type: &str, nullable: bool) -> TableColumn {
        TableColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable,
            default: None,
            primary_key: false,
            unique: false,
        }
    }

    fn projects() -> TableSchema {
        let mut table = TableSchema::new("portfolio_projects");
        table.add_column(column("organisation_id", "UUID", false));
        table.add_column(column("id", "UUID", false));
        table.add_column(column("slug", "TEXT", false));
        table.add_column(column("publish_at", "DATE", true));
        table.add_column(column("deleted_at", "TIMESTAMPTZ", true));
        table.set_key(KeyPart::parts(&[
            ("organisation_id", Some(ShardKind::Hash)),
            ("id", Some(ShardKind::Asc)),
        ]));
        table.add_index(TableIndex {
            name: "portfolio_projects_by_slug".to_owned(),
            columns: vec!["organisation_id".to_owned(), "slug".to_owned()],
            unique: true,
            condition: Some("deleted_at IS NULL".to_owned()),
            key: KeyPart::parts(&[
                ("organisation_id", Some(ShardKind::Hash)),
                ("slug", Some(ShardKind::Asc)),
            ]),
            include: Vec::new(),
            constraint: false,
        });
        table
    }

    #[test]
    fn a_table_and_its_indexes_are_the_sql_a_person_writes() {
        assert_eq!(
            create_all(&projects()),
            [
                "CREATE TABLE IF NOT EXISTS portfolio_projects (\n    \
                 organisation_id UUID NOT NULL,\n    \
                 id UUID NOT NULL,\n    \
                 slug TEXT NOT NULL,\n    \
                 publish_at DATE,\n    \
                 deleted_at TIMESTAMPTZ,\n    \
                 PRIMARY KEY (organisation_id HASH, id ASC)\n);",
                "CREATE UNIQUE INDEX IF NOT EXISTS portfolio_projects_by_slug\n    \
                 ON portfolio_projects (organisation_id HASH, slug ASC) WHERE deleted_at IS NULL;",
            ]
        );
    }

    #[test]
    fn hashed_columns_are_one_part_and_an_order_says_where_empty_values_go() {
        let index = TableIndex {
            name: "threads_by_activity".to_owned(),
            columns: Vec::new(),
            unique: false,
            condition: None,
            key: KeyPart::ordered_parts(&[
                ("kind", Some(ShardKind::Hash), None),
                ("entity_id", Some(ShardKind::Hash), None),
                ("at", Some(ShardKind::Desc), Some(Nulls::Last)),
            ]),
            include: vec!["title".to_owned()],
            constraint: false,
        };
        assert_eq!(
            create_index("threads", &index),
            "CREATE INDEX IF NOT EXISTS threads_by_activity\n    \
             ON threads ((kind, entity_id) HASH, at DESC NULLS LAST) INCLUDE (title);"
        );
    }

    #[test]
    fn a_table_out_of_colocation_split_into_tablets_says_so_after_its_columns() {
        let mut table = projects();
        table.set_colocated(false);
        table.set_tablets(8);

        let created = &create_all(&table)[0];

        assert!(
            created.ends_with("\n) WITH (COLOCATION = false) SPLIT INTO 8 TABLETS;"),
            "{created}"
        );
    }

    #[test]
    fn a_table_as_the_database_places_it_says_nothing_of_where() {
        let created = &create_all(&projects())[0];

        assert!(created.ends_with("\n);"), "{created}");
    }

    #[test]
    fn a_writer_is_made_when_absent_and_granted_every_row() {
        let mut table = projects();
        table.add_writer("importer");

        let statements = create_all(&table);

        assert_eq!(
            &statements[statements.len() - 2..],
            [
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'importer') \
                 THEN CREATE ROLE importer NOLOGIN; END IF; EXCEPTION WHEN duplicate_object THEN \
                 NULL; END $$;",
                "GRANT SELECT, INSERT, UPDATE, DELETE ON portfolio_projects TO importer;",
            ]
        );
    }

    #[test]
    fn the_guard_accepts_what_is_rendered() {
        let mut read = projects();
        read.add_reader("www", "publish_at IS NOT NULL AND deleted_at IS NULL");
        read.add_writer("importer");
        read.set_colocated(false);
        read.set_tablets(4);
        let statements = create_all(&read).join("\n\n");
        MigrationGuard::new(1_000, Lints::default(), &[])
            .inspect_sql(&[("0001_portfolio_projects", statements.as_str())])
            .expect("the guard accepts a rendered table");
    }
}
