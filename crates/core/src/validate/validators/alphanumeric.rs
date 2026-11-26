//! Alphanumeric validation.

use crate::validate::ValidationError;

pub fn validate_alphanumeric(field: &str, value: &str) -> Result<(), ValidationError> {
    if !value.is_empty() && !value.chars().all(|c| c.is_alphanumeric()) {
        return Err(ValidationError::invalid_format(
            field,
            "must contain only letters and numbers",
        ));
    }
    Ok(())
}
