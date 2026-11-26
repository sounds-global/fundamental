//! Conflict error category.

use crate::Error;

/// Marker trait for conflict errors.
pub trait ConflictError: Error {}
