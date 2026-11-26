//! Minimum-length validator.

use crate::validate::ValidationError;

pub fn validate_min_length(field: &str, value: &str, min: usize) -> Result<(), ValidationError> {
    let len = value.chars().count();
    if len < min {
        return Err(ValidationError::too_short(field, min, len));
    }

    Ok(())
}
