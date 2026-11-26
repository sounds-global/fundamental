use crate::KeyError;

/// Result type for key management operations.
pub type KeyResult<T> = Result<T, KeyError>;
