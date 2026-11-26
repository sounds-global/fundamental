use crate::ToRow;

/// Helper to build DELETE CQL statements.
pub(crate) struct DeleteBuilder {
    keyspace: String,
    table: String,
    where_columns: Vec<String>,
    if_exists: bool,
}

impl DeleteBuilder {
    pub(crate) fn new(keyspace: impl Into<String>, table: impl Into<String>) -> Self {
        Self {
            keyspace: keyspace.into(),
            table: table.into(),
            where_columns: Vec::new(),
            if_exists: false,
        }
    }

    pub(crate) fn where_columns_from<T: ToRow>(mut self) -> Self {
        let mut cols: Vec<String> = T::partition_key_columns()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        cols.extend(T::clustering_key_columns().iter().map(|s| (*s).to_string()));
        self.where_columns = cols;
        self
    }

    pub(crate) fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }

    pub(crate) fn build(&self) -> String {
        let where_clause: Vec<String> = self
            .where_columns
            .iter()
            .map(|col| format!("{} = ?", col))
            .collect();

        let mut cql = format!(
            "DELETE FROM {} WHERE {}",
            super::qualify(&self.keyspace, &self.table),
            where_clause.join(" AND ")
        );

        if self.if_exists {
            cql.push_str(" IF EXISTS");
        }

        cql
    }
}
