//! Maximum-value validation.

use crate::validate::{ParamValue, ValidationError};

pub fn validate_max<T: PartialOrd + Copy + Into<ParamValue>>(
    field: &str,
    value: T,
    max: T,
) -> Result<(), ValidationError> {
    if value > max {
        return Err(ValidationError::too_large(field, max, value));
    }
    Ok(())
}
