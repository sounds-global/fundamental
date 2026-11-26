//! A table schema as the YCQL a person writes for it.

use crate::migration::TableSchema;

/// `CREATE TABLE` for a YCQL `table`, with its clustering order spelled out so a descending
/// column survives a read back.
#[must_use]
pub fn create_cql_table(table: &TableSchema) -> String {
    let mut lines: Vec<String> = table
        .column_order
        .iter()
        .filter_map(|name| table.columns.get(name))
        .map(|column| format!("    {} {}", column.name, column.data_type))
        .collect();
    let partition = table
        .partition_key
        .clone()
        .unwrap_or_else(|| table.primary_key.clone());
    let clustering = table.clustering_key.clone().unwrap_or_default();
    let mut key = vec![format!("({})", partition.join(", "))];
    key.extend(clustering.iter().map(|(column, _)| column.clone()));
    lines.push(format!("    PRIMARY KEY ({})", key.join(", ")));
    let mut statement = format!(
        "CREATE TABLE IF NOT EXISTS {} (\n{}\n)",
        table.name,
        lines.join(",\n")
    );
    if !clustering.is_empty() {
        let orders: Vec<String> = clustering
            .iter()
            .map(|(column, order)| format!("{column} {}", order.as_cql()))
            .collect();
        statement.push_str(&format!(
            " WITH CLUSTERING ORDER BY ({})",
            orders.join(", ")
        ));
    }
    statement
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::cql::ClusteringOrder;

    use super::create_cql_table;
    use crate::migration::{MigrationDatabase, TableColumn, TableSchema};

    fn column(name: &str, data_type: &str, primary_key: bool) -> TableColumn {
        TableColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            default: None,
            primary_key,
            unique: false,
        }
    }

    #[test]
    fn a_companion_lives_in_its_records_partition_in_the_records_order() {
        let mut notices = TableSchema::new("notices");
        notices.add_column(column("organisation_id", "uuid", true));
        notices.add_column(column("id", "bigint", true));
        notices.add_column(column("title", "text", false));
        notices.set_primary_key(vec!["organisation_id".to_owned(), "id".to_owned()]);
        notices.set_partition_key(vec!["organisation_id".to_owned()]);
        notices.set_clustering_key(vec![("id".to_owned(), ClusteringOrder::Desc)]);

        assert_eq!(
            create_cql_table(&notices.translations(MigrationDatabase::CqlDb)),
            "CREATE TABLE IF NOT EXISTS notices_i18n (\n    organisation_id uuid,\n    id bigint,\n    \
             field text,\n    locale text,\n    value text,\n    source_hash text,\n    \
             translation_hash text,\n    checked_by text,\n    checked_at bigint,\n    \
             PRIMARY KEY ((organisation_id), id, field, locale)\n) \
             WITH CLUSTERING ORDER BY (id DESC, field ASC, locale ASC)"
        );
    }

    #[test]
    fn a_table_with_no_clustering_column_has_no_order() {
        let mut tags = TableSchema::new("tags");
        tags.add_column(column("name", "text", true));
        tags.set_primary_key(vec!["name".to_owned()]);
        tags.set_partition_key(vec!["name".to_owned()]);

        assert_eq!(
            create_cql_table(&tags),
            "CREATE TABLE IF NOT EXISTS tags (\n    name text,\n    PRIMARY KEY ((name))\n)"
        );
    }
}
