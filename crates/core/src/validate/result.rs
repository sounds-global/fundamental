//! The [`Result`] alias returned by validation operations.

use super::ValidationErrors;

/// Result type for validation operations.
pub type ValidationResult<T> = Result<T, ValidationErrors>;
