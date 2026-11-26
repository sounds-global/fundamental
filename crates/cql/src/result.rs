//! The crate's [`Result`] alias.

use crate::CqlError;

/// Result type for YCQL operations.
pub type CqlResult<T> = Result<T, CqlError>;
