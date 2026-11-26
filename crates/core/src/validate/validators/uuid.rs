//! UUID validation.

use crate::validate::ValidationError;

pub fn validate_uuid(field: &str, value: &str) -> Result<(), ValidationError> {
    if !value.is_empty() && !is_valid_uuid(value) {
        return Err(ValidationError::invalid_uuid(field));
    }
    Ok(())
}

fn is_valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}
