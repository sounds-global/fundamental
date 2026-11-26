//! Typestate marker types tracking whether a partition key has been bound.

/// Type state: partition key has NOT been specified.
///
/// Queries in this state cannot be executed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPartitionKey;

/// Type state: partition key HAS been specified.
///
/// Queries in this state can be executed.
#[derive(Debug, Clone, Copy, Default)]
pub struct HasPartitionKey;
