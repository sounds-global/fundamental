//! Identifier validation.

use crate::validate::ValidationError;

#[cfg(not(feature = "std"))]
use alloc::format;

/// Validates that a string is a valid identifier.
pub fn validate_identifier(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::required(field));
    }

    let first = value.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(ValidationError::invalid_format(
            field,
            "must start with a letter or underscore",
        ));
    }

    for c in value.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' {
            return Err(ValidationError::invalid_format(
                field,
                format!(
                    "invalid character '{}': only letters, numbers, and underscores are allowed",
                    c
                ),
            ));
        }
    }

    Ok(())
}
