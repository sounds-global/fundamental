//! Builder for parameterized `INSERT` statements.

use crate::ToRow;

pub(crate) struct InsertBuilder {
    table: String,
    columns: Vec<String>,
    returning: Option<String>,
}

impl InsertBuilder {
    pub(crate) fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            returning: None,
        }
    }

    pub(crate) fn columns_from<T: ToRow>(mut self) -> Self {
        self.columns = T::columns().iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub(crate) fn returning(mut self, cols: impl Into<String>) -> Self {
        self.returning = Some(cols.into());
        self
    }

    pub(crate) fn build(&self) -> String {
        let placeholders: Vec<String> = (1..=self.columns.len())
            .map(|i| format!("${}", i))
            .collect();

        let mut sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table,
            self.columns.join(", "),
            placeholders.join(", ")
        );

        if let Some(ref returning) = self.returning {
            sql.push_str(" RETURNING ");
            sql.push_str(returning);
        }

        sql
    }
}
