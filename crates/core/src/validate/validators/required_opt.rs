//! Presence validation for optional fields.

use crate::validate::ValidationError;

pub fn validate_required_opt<T>(field: &str, value: &Option<T>) -> Result<(), ValidationError> {
    if value.is_none() {
        return Err(ValidationError::required(field));
    }

    Ok(())
}
