//! ASCII validation.

use crate::validate::ValidationError;

pub fn validate_ascii(field: &str, value: &str) -> Result<(), ValidationError> {
    if !value.is_empty() && !value.is_ascii() {
        return Err(ValidationError::invalid_format(
            field,
            "must contain only ASCII characters",
        ));
    }
    Ok(())
}
