//! Builder for parameterized `UPDATE` statements.

use crate::ToRow;

pub(crate) struct UpdateBuilder {
    table: String,
    columns: Vec<String>,
    where_clause: Option<String>,
    returning: Option<String>,
}

impl UpdateBuilder {
    pub(crate) fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            where_clause: None,
            returning: None,
        }
    }

    pub(crate) fn columns_from<T: ToRow>(mut self) -> Self {
        self.columns = T::columns_without_pk()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        self
    }

    /// Adds a WHERE matching every key column, with placeholders continuing from the SET list.
    pub(crate) fn where_pk(mut self, pk_columns: &[&str]) -> Self {
        let first = self.columns.len() + 1;
        self.where_clause = Some(
            pk_columns
                .iter()
                .enumerate()
                .map(|(offset, column)| format!("{} = ${}", column, first + offset))
                .collect::<Vec<_>>()
                .join(" AND "),
        );
        self
    }

    pub(crate) fn returning(mut self, cols: impl Into<String>) -> Self {
        self.returning = Some(cols.into());
        self
    }

    pub(crate) fn build(&self) -> String {
        let set_clauses: Vec<String> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, col)| format!("{} = ${}", col, i + 1))
            .collect();

        let mut sql = format!("UPDATE {} SET {}", self.table, set_clauses.join(", "));

        if let Some(ref where_clause) = self.where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(where_clause);
        }

        if let Some(ref returning) = self.returning {
            sql.push_str(" RETURNING ");
            sql.push_str(returning);
        }

        sql
    }
}
