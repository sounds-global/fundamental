//! Batch kinds.

use scylla::statement::batch::BatchType as CqlBatchType;

/// The type of batch operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchType {
    /// Logged batch: all statements succeed or fail together.
    Logged,

    /// Unlogged batch: faster, but partial failures are possible.
    Unlogged,

    /// Counter batch, for counter increments and decrements only.
    Counter,
}

impl From<BatchType> for CqlBatchType {
    fn from(bt: BatchType) -> Self {
        match bt {
            BatchType::Logged => CqlBatchType::Logged,
            BatchType::Unlogged => CqlBatchType::Unlogged,
            BatchType::Counter => CqlBatchType::Counter,
        }
    }
}
