use crate::Validate;
use crate::validate::ValidationResult;

/// Unit type for actions that don't require input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoInput;

impl Validate for NoInput {
    fn validate(&self) -> ValidationResult<()> {
        Ok(())
    }
}
