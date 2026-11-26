//! Fluent builder for accumulating validation errors across many fields.

use super::{FieldValidator, Validate, ValidationError, ValidationErrors, ValidationResult};

#[cfg(not(feature = "std"))]
use alloc::format;

/// Context for building validation results.
///
/// # Examples
///
/// ```rust
/// use fundamental_core::validate::ValidationContext;
///
/// fn validate_user(email: &str, name: &str, age: u8) -> Result<(), fundamental_core::validate::ValidationErrors> {
///     let mut ctx = ValidationContext::new();
///
///     ctx.field("email")
///         .required(email)
///         .email(email);
///
///     ctx.field("name")
///         .required(name)
///         .length(name, 1, 100);
///
///     ctx.field("age")
///         .range(age, 13, 120);
///
///     ctx.into_result()
/// }
/// ```
#[derive(Debug, Default)]
pub struct ValidationContext {
    /// The errors accumulated so far across all validated fields.
    pub errors: ValidationErrors,
}

impl ValidationContext {
    /// Creates a new validation context.
    pub fn new() -> Self {
        Self {
            errors: ValidationErrors::new(),
        }
    }

    /// Starts validating a field.
    pub fn field<'a>(&'a mut self, name: &'a str) -> FieldValidator<'a> {
        FieldValidator {
            ctx: self,
            field: name,
        }
    }

    /// Adds a custom error.
    pub fn add_error<E: Into<ValidationError>>(&mut self, error: E) {
        self.errors.add(error);
    }

    /// Validates a nested value that implements Validate.
    pub fn validate_nested<T: Validate>(&mut self, field: &str, value: &T) {
        if let Err(nested_errors) = value.validate() {
            self.errors.merge_with_prefix(field, nested_errors);
        }
    }

    /// Validates an optional nested value.
    pub fn validate_nested_opt<T: Validate>(&mut self, field: &str, value: &Option<T>) {
        if let Some(v) = value {
            self.validate_nested(field, v);
        }
    }

    /// Validates a collection of nested values.
    pub fn validate_nested_vec<T: Validate>(&mut self, field: &str, values: &[T]) {
        for (i, value) in values.iter().enumerate() {
            let indexed_field = format!("{}[{}]", field, i);
            if let Err(nested_errors) = value.validate() {
                self.errors.merge_with_prefix(&indexed_field, nested_errors);
            }
        }
    }

    /// Returns whether any errors have been collected.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns the number of errors.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Merges errors from another [`ValidationErrors`], leaving their field paths unchanged.
    pub fn merge_errors(&mut self, other: ValidationErrors) {
        self.errors.merge(other);
    }

    /// Merges errors from another ValidationErrors, prefixing all field paths.
    pub fn merge_errors_with_prefix(&mut self, prefix: &str, other: ValidationErrors) {
        self.errors.merge_with_prefix(prefix, other);
    }

    /// Converts into a Result.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn into_result(self) -> ValidationResult<()> {
        self.errors.into_result()
    }

    /// Returns the collected errors.
    pub fn into_errors(self) -> ValidationErrors {
        self.errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_context() {
        let mut ctx = ValidationContext::new();

        ctx.field("email").required("").email("invalid");
        ctx.field("name").length("ab", 3, 10);
        ctx.field("age").range(5u8, 13, 120);

        assert!(ctx.has_errors());
        assert_eq!(ctx.error_count(), 4); // required, invalid_email, too_short, too_small
    }

    #[test]
    fn test_nested_validation() {
        struct Inner {
            value: String,
        }

        impl Validate for Inner {
            fn validate(&self) -> Result<(), ValidationErrors> {
                let mut ctx = ValidationContext::new();
                ctx.field("value").required(&self.value);
                ctx.into_result()
            }
        }

        let mut ctx = ValidationContext::new();
        ctx.validate_nested("inner", &Inner { value: "".into() });

        let errors = ctx.into_errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors.first().unwrap().field, Some("inner.value".into()));
    }

    #[test]
    fn test_validate_vec() {
        struct Item {
            name: String,
        }

        impl Validate for Item {
            fn validate(&self) -> Result<(), ValidationErrors> {
                let mut ctx = ValidationContext::new();
                ctx.field("name").required(&self.name);
                ctx.into_result()
            }
        }

        let items = vec![
            Item {
                name: "valid".into(),
            },
            Item { name: "".into() },
            Item { name: "".into() },
        ];

        let result = items.validate();
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(
            errors.errors()[0]
                .field
                .as_ref()
                .unwrap()
                .starts_with("[1]")
        );
        assert!(
            errors.errors()[1]
                .field
                .as_ref()
                .unwrap()
                .starts_with("[2]")
        );
    }

    #[test]
    fn test_field_validator_chaining() {
        let mut ctx = ValidationContext::new();

        ctx.field("username")
            .required("test")
            .min_length("test", 3)
            .max_length("test", 20)
            .alphanumeric("test");

        assert!(!ctx.has_errors());
    }

    #[test]
    fn test_conditional_validation() {
        let mut ctx = ValidationContext::new();
        let require_email = false;

        ctx.field("email").when(require_email, |v| v.required(""));

        assert!(!ctx.has_errors());
    }
}
