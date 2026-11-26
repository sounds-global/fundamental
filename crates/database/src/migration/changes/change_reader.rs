//! The [`ChangeReaderChange`] schema change.

/// A role that reads only some of a table's rows: `new` as the model says, `old` as the table
/// has it; `None` is no such reader.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeReaderChange {
    /// Table name.
    pub table_name: String,
    /// The reading role.
    pub role: String,
    /// The rows the table lets the role see, as SQL, if it lets it read at all.
    pub old: Option<String>,
    /// The rows the model lets the role see, as SQL, if it names the role at all.
    pub new: Option<String>,
}

impl ChangeReaderChange {
    /// What the change does, in words: the rows the role is let read, and the rows it reads.
    #[must_use]
    pub fn description(&self) -> String {
        let reads = self
            .old
            .as_ref()
            .map_or_else(|| "none".to_owned(), |when| format!("where {when}"));
        match &self.new {
            Some(when) => format!(
                "Let '{}' read the rows of '{}' where {when} (it reads {reads})",
                self.role, self.table_name
            ),
            None => format!(
                "Let '{}' read no rows of '{}' (it reads {reads})",
                self.role, self.table_name
            ),
        }
    }
}
