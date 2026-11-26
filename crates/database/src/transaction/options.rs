//! Transaction configuration options.

use super::IsolationLevel;

/// Options for transaction behavior.
///
/// Not [`Default`], so the isolation level is always stated.
#[derive(Debug, Clone)]
pub struct TransactionOptions {
    /// The isolation level.
    pub isolation_level: IsolationLevel,
    /// Whether the transaction is read-only.
    pub read_only: bool,
    /// Whether to use a deferrable transaction (PostgreSQL wire).
    pub deferrable: bool,
    /// Transaction timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl TransactionOptions {
    /// Creates transaction options at `level`.
    #[must_use]
    pub fn new(level: IsolationLevel) -> Self {
        Self {
            isolation_level: level,
            read_only: false,
            deferrable: false,
            timeout_ms: None,
        }
    }

    /// Sets the isolation level.
    #[must_use]
    pub fn isolation_level(mut self, level: IsolationLevel) -> Self {
        self.isolation_level = level;
        self
    }

    /// Sets the transaction as read-only.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Sets the transaction as deferrable.
    #[must_use]
    pub fn deferrable(mut self) -> Self {
        self.deferrable = true;
        self
    }

    /// Sets a timeout.
    #[must_use]
    pub fn timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}
