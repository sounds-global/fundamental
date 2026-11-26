//! Builder for parameterized `UPSERT` (`INSERT ... ON CONFLICT`) statements.

use crate::ToRow;

pub(crate) struct UpsertBuilder {
    table: String,
    columns: Vec<String>,
    // A key, not a column: a `UNIQUE` constraint is frequently composite.
    conflict_columns: Vec<String>,
    do_nothing: bool,
    keep_stored: Option<String>,
    guard: Option<String>,
    returning: Option<String>,
}

impl UpsertBuilder {
    pub(crate) fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            conflict_columns: Vec::new(),
            do_nothing: false,
            keep_stored: None,
            guard: None,
            returning: None,
        }
    }

    pub(crate) fn columns_from<T: ToRow>(mut self) -> Self {
        self.columns = T::columns().iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Sets the `ON CONFLICT` target: the constraint's columns, in its declared order.
    pub(crate) fn on_conflict(mut self, columns: &[&str]) -> Self {
        self.conflict_columns = columns.iter().map(|column| (*column).to_string()).collect();
        self
    }

    /// Makes the conflict clause `DO NOTHING`, an idempotent insert.
    pub(crate) fn do_nothing(mut self) -> Self {
        self.do_nothing = true;
        self
    }

    /// Rewrites the stored key onto itself on conflict, so the row is kept but locked and returned; `stored` is the table's unqualified name.
    pub(crate) fn keep_stored(mut self, stored: impl Into<String>) -> Self {
        self.keep_stored = Some(stored.into());
        self
    }

    /// Guards the `DO UPDATE` with `WHERE <predicate>`, an already-validated fragment; ignored with `do_nothing`.
    pub(crate) fn guard(mut self, predicate: impl Into<String>) -> Self {
        self.guard = Some(predicate.into());
        self
    }

    pub(crate) fn returning(mut self, cols: impl Into<String>) -> Self {
        self.returning = Some(cols.into());
        self
    }

    /// Builds the UPSERT SQL, assigning every non-primary-key column from `EXCLUDED`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn build<T: ToRow>(&self) -> String {
        let placeholders: Vec<String> = (1..=self.columns.len())
            .map(|i| format!("${}", i))
            .collect();

        let update_columns: Vec<String> = T::columns_without_pk()
            .iter()
            .map(|col| format!("{} = EXCLUDED.{}", col, col))
            .collect();

        let mut sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table,
            self.columns.join(", "),
            placeholders.join(", ")
        );

        if !self.conflict_columns.is_empty() {
            let target = self.conflict_columns.join(", ");
            if let Some(ref stored) = self.keep_stored {
                let first = &self.conflict_columns[0];
                sql.push_str(&format!(
                    " ON CONFLICT ({}) DO UPDATE SET {} = {}.{}",
                    target, first, stored, first
                ));
            } else if self.do_nothing {
                sql.push_str(&format!(" ON CONFLICT ({}) DO NOTHING", target));
            } else {
                sql.push_str(&format!(
                    " ON CONFLICT ({}) DO UPDATE SET {}",
                    target,
                    update_columns.join(", ")
                ));
                if let Some(ref guard) = self.guard {
                    sql.push_str(&format!(" WHERE {}", guard));
                }
            }
        }

        if let Some(ref returning) = self.returning {
            sql.push_str(" RETURNING ");
            sql.push_str(returning);
        }

        sql
    }
}
