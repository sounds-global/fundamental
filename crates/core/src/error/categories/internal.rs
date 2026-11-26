//! Internal error category.

use crate::Error;

/// Marker trait for internal errors.
pub trait InternalError: Error {}
