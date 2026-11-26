//! String length validation.

use crate::validate::ValidationError;

pub fn validate_length(
    field: &str,
    value: &str,
    min: usize,
    max: usize,
) -> Result<(), ValidationError> {
    let len = value.chars().count();
    if len < min {
        return Err(ValidationError::too_short(field, min, len));
    } else if len > max {
        return Err(ValidationError::too_long(field, max, len));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_length() {
        assert!(validate_length("name", "abc", 1, 3).is_ok());
        assert!(validate_length("name", "a", 2, 0).is_err());
        assert!(validate_length("name", "abcdefghijk", 0, 1).is_err());
    }
}
