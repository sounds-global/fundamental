//! Not-found error category.

use crate::Error;

/// Marker trait for not found errors.
pub trait NotFoundError: Error {}
