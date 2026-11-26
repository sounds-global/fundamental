//! Inclusive bounds validation for comparable values.

use crate::validate::{ParamValue, ValidationError};

pub fn validate_range<T: PartialOrd + Copy + Into<ParamValue>>(
    field: &str,
    value: T,
    min: T,
    max: T,
) -> Result<(), ValidationError> {
    if value < min {
        return Err(ValidationError::too_small(field, min, value));
    } else if value > max {
        return Err(ValidationError::too_large(field, max, value));
    }

    Ok(())
}
