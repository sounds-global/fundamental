//! Authorization error category.

use crate::Error;

/// Marker trait for authorization errors.
pub trait AuthorizationError: Error {}
