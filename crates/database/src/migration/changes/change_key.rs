//! The [`ChangeKeyChange`] schema change, which needs a new table.

use crate::migration::KeyPart;

/// A SQL table's key changed: its columns, their order, or how each part is split.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeKeyChange {
    /// Table name.
    pub table_name: String,
    /// The key the table has.
    pub old_key: Vec<KeyPart>,
    /// The key declared.
    pub new_key: Vec<KeyPart>,
}
