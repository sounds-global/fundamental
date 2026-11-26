//! The parameter type for a slice that takes no parameters.

use fundamental_core::validate::{Validate, ValidationResult};
use serde::{Deserialize, Serialize};

/// The parameter type for a slice scoped by the principal alone: absent, null or `{}` on the wire.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NoParams {}

impl Validate for NoParams {
    fn validate(&self) -> ValidationResult<()> {
        Ok(())
    }
}
