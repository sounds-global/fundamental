//! Builder for parameterized `DELETE` statements.

pub(crate) struct DeleteBuilder {
    table: String,
    where_clause: Option<String>,
}

impl DeleteBuilder {
    pub(crate) fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            where_clause: None,
        }
    }

    /// Adds a WHERE matching every key column, in key order.
    pub(crate) fn where_pk(mut self, pk_columns: &[&str]) -> Self {
        self.where_clause = Some(
            pk_columns
                .iter()
                .enumerate()
                .map(|(offset, column)| format!("{} = ${}", column, offset + 1))
                .collect::<Vec<_>>()
                .join(" AND "),
        );
        self
    }

    pub(crate) fn build(&self) -> String {
        let mut sql = format!("DELETE FROM {}", self.table);

        if let Some(ref where_clause) = self.where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(where_clause);
        }

        sql
    }
}
