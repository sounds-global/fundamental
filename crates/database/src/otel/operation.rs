//! The [`DbOperation`] enum, the `db.operation` OpenTelemetry attribute.

/// Database operation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbOperation {
    /// SELECT query
    Select,
    /// INSERT statement
    Insert,
    /// UPDATE statement
    Update,
    /// DELETE statement
    Delete,
    /// Batch operation
    Batch,
    /// Transaction begin
    TransactionBegin,
    /// Transaction commit
    TransactionCommit,
    /// Transaction rollback
    TransactionRollback,
    /// Other operation
    Other(&'static str),
}

impl DbOperation {
    /// Returns the operation name as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Select => "select",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Batch => "batch",
            Self::TransactionBegin => "transaction_begin",
            Self::TransactionCommit => "transaction_commit",
            Self::TransactionRollback => "transaction_rollback",
            Self::Other(s) => s,
        }
    }
}

impl std::fmt::Display for DbOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
