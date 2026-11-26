//! Transaction isolation levels.

/// Transaction isolation levels.
///
/// No [`Default`]: engines differ in what they default to, so a caller always chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsolationLevel {
    /// Read uncommitted changes from other transactions.
    ///
    /// Allows dirty reads.
    ReadUncommitted,

    /// Only read committed changes.
    ///
    /// Prevents dirty reads but allows non-repeatable reads.
    ReadCommitted,

    /// Repeatable reads within the transaction.
    ///
    /// Prevents dirty reads and non-repeatable reads.
    RepeatableRead,

    /// Full serializability.
    ///
    /// On YSQL a hot row arrives as `40P01` deadlock rather than a `40001` restart, and
    /// `SKIP LOCKED` is ignored.
    Serializable,
}

impl IsolationLevel {
    /// Returns the SQL representation of this isolation level.
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::ReadUncommitted => "READ UNCOMMITTED",
            Self::ReadCommitted => "READ COMMITTED",
            Self::RepeatableRead => "REPEATABLE READ",
            Self::Serializable => "SERIALIZABLE",
        }
    }
}
