//! Builder for counter `UPDATE` CQL statements (atomic increments, optional TTL).

/// Helper to build counter UPDATE CQL statements.
///
/// `UPDATE ks.tbl SET counter_col = counter_col + ? WHERE key = ?`
pub(crate) struct CounterUpdateBuilder {
    keyspace: String,
    table: String,
    counter_column: String,
    where_columns: Vec<String>,
    ttl: Option<i32>,
}

impl CounterUpdateBuilder {
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
            ttl: None,
        }
    }

    pub(crate) fn where_columns(mut self, columns: &[&str]) -> Self {
        self.where_columns = columns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub(crate) fn ttl(mut self, seconds: i32) -> Self {
        self.ttl = Some(seconds);
        self
    }

    pub(crate) fn build(&self) -> String {
        let where_clause: Vec<String> = self
            .where_columns
            .iter()
            .map(|col| format!("{} = ?", col))
            .collect();

        let mut cql = String::new();

        cql.push_str("UPDATE ");
        cql.push_str(&self.keyspace);
        cql.push('.');
        cql.push_str(&self.table);

        if let Some(ttl) = self.ttl {
            cql.push_str(&format!(" USING TTL {}", ttl));
        }

        cql.push_str(&format!(
            " SET {} = {} + ?",
            self.counter_column, self.counter_column
        ));
        cql.push_str(" WHERE ");
        cql.push_str(&where_clause.join(" AND "));

        cql
    }
}
