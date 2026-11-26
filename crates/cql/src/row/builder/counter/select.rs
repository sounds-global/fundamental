//! Builder for counter `SELECT` CQL statements.

/// Helper to build counter SELECT CQL statements for reading counter values.
pub(crate) struct CounterSelectBuilder {
    keyspace: String,
    table: String,
    counter_column: String,
    where_columns: Vec<String>,
}

impl CounterSelectBuilder {
    pub(crate) fn new(
        keyspace: impl Into<String>,
        table: impl Into<String>,
        counter_column: impl Into<String>,
    ) -> Self {
        Self {
            keyspace: keyspace.into(),
            table: table.into(),
            counter_column: counter_column.into(),
            where_columns: Vec::new(),
        }
    }

    pub(crate) fn where_columns(mut self, columns: &[&str]) -> Self {
        self.where_columns = columns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub(crate) fn build(&self) -> String {
        let where_clause: Vec<String> = self
            .where_columns
            .iter()
            .map(|col| format!("{} = ?", col))
            .collect();

        format!(
            "SELECT {} FROM {}.{} WHERE {}",
            self.counter_column,
            self.keyspace,
            self.table,
            where_clause.join(" AND ")
        )
    }
}
