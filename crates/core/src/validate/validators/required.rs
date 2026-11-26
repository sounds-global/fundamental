//! Presence validation for string fields.

use crate::validate::ValidationError;

pub fn validate_required(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::required(field));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_required() {
        assert!(validate_required("email", "test@example.com").is_ok());
        assert!(validate_required("email", "").is_err());
        assert!(validate_required("email", "   ").is_err());
    }
}
