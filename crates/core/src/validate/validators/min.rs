//! Minimum-value validator.

use crate::validate::{ParamValue, ValidationError};

pub fn validate_min<T: PartialOrd + Copy + Into<ParamValue>>(
    field: &str,
    value: T,
    min: T,
) -> Result<(), ValidationError> {
    if value < min {
        return Err(ValidationError::too_small(field, min, value));
    }
    Ok(())
}
