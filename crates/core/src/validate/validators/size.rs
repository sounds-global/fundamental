//! Element-count validation for slices.

use crate::validate::ValidationError;

/// Validates collection size.
pub fn validate_size<T>(
    field: &str,
    value: &[T],
    min: usize,
    max: usize,
) -> Result<(), ValidationError> {
    let len = value.len();
    if len < min {
        return Err(ValidationError::too_small(field, min, len).with_param("unit", "items"));
    } else if len > max {
        return Err(ValidationError::too_large(field, max, len).with_param("unit", "items"));
    }

    Ok(())
}
