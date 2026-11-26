//! Maximum-length validator.

use crate::validate::ValidationError;

pub fn validate_max_length(field: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    let len = value.chars().count();
    if len > max {
        return Err(ValidationError::too_long(field, max, len));
    }

    Ok(())
}
