//! Result rows returned by queries.

use std::sync::Arc;

use super::Value;

/// A single result row.
#[derive(Debug, Clone)]
pub struct Row {
    columns: Arc<[String]>,
    values: Vec<Value>,
}

impl Row {
    pub(crate) fn new(columns: Arc<[String]>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    /// Builds a row from column names and values, outside a query.
    ///
    /// For rows carried across a process boundary.
    #[must_use]
    pub fn from_parts(columns: Vec<String>, values: Vec<Value>) -> Self {
        Self::new(Arc::from(columns), values)
    }

    /// Returns the value at the given column index.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    /// Returns the value for the given column name.
    pub fn get_named(&self, name: &str) -> Option<&Value> {
        let index = self.columns.iter().position(|c| c == name)?;
        self.values.get(index)
    }

    /// Returns the column names.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Returns the number of columns.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true if the row has no columns.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Consumes the row, returning its values.
    pub fn into_values(self) -> Vec<Value> {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::{Row, Value};

    #[test]
    fn from_parts_round_trips_positional_and_named_access() {
        let row = Row::from_parts(
            vec!["id".to_owned(), "title".to_owned()],
            vec![Value::Text("a".to_owned()), Value::Text("hello".to_owned())],
        );
        assert_eq!(row.get(1), Some(&Value::Text("hello".to_owned())));
        assert_eq!(row.get_named("id"), Some(&Value::Text("a".to_owned())));
        assert_eq!(row.columns(), ["id", "title"]);
        assert_eq!(row.len(), 2);
    }
}
