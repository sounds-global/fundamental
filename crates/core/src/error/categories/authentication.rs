//! Authentication error category.

use crate::Error;

/// Marker trait for authentication errors.
pub trait AuthenticationError: Error {}
