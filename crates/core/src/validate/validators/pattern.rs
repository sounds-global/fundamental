//! Compiled-regex pattern validator.

use crate::validate::ValidationError;

pub fn validate_pattern(
    field: &str,
    value: &str,
    pattern: &regex::Regex,
) -> Result<(), ValidationError> {
    if !value.is_empty() && !pattern.is_match(value) {
        return Err(ValidationError::pattern_mismatch(field, pattern.as_str()));
    }
    Ok(())
}
