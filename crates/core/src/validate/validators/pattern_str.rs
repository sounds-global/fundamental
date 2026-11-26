//! Regex-pattern validation against a runtime pattern string.

use crate::validate::ValidationError;

pub fn validate_pattern_str(
    field: &str,
    value: &str,
    pattern: &str,
) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Ok(());
    }

    match regex::Regex::new(pattern) {
        Ok(re) => {
            if !re.is_match(value) {
                return Err(ValidationError::pattern_mismatch(field, pattern));
            }
        }
        Err(_) => {
            return Err(ValidationError::invalid_format(
                field,
                format!("invalid regex pattern: {}", pattern),
            ));
        }
    }

    Ok(())
}
