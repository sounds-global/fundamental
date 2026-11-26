//! Allowed-value (enumeration) validator.

use crate::validate::ValidationError;

/// Validates that the value is one of the allowed values.
pub fn validate_one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), ValidationError> {
    if !value.is_empty() && !allowed.contains(&value) {
        return Err(ValidationError::not_in_list(field, allowed));
    }
    Ok(())
}
