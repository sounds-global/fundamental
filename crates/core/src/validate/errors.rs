//! A collection of validation failures reported together.

use super::ValidationError;
use crate::Error;

use core::fmt;
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(feature = "std")]
use std::borrow::Cow;

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec, vec::Vec};

/// A collection of validation errors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Creates an empty validation errors collection.
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Creates a validation errors collection with a single error.
    pub fn single<E: Into<ValidationError>>(error: E) -> Self {
        Self {
            errors: vec![error.into()],
        }
    }

    /// Adds an error to the collection.
    pub fn add<E: Into<ValidationError>>(&mut self, error: E) {
        self.errors.push(error.into());
    }

    /// Adds an error for a specific field.
    pub fn add_for_field(
        &mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.add(ValidationError::new(Some(field.into()), code, message));
    }

    /// Returns whether there are any errors.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the number of errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Returns all errors.
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Consumes self and returns the errors.
    pub fn into_errors(self) -> Vec<ValidationError> {
        self.errors
    }

    /// Returns errors for a specific field.
    pub fn errors_for_field(&self, field: &str) -> Vec<&ValidationError> {
        self.errors
            .iter()
            .filter(|e| e.field.as_deref() == Some(field))
            .collect()
    }

    /// Returns errors that start with a given path prefix.
    pub fn errors_for_prefix(&self, prefix: &str) -> Vec<&ValidationError> {
        let prefix_dot = format!("{}.", prefix);
        self.errors
            .iter()
            .filter(|e| {
                e.field
                    .as_ref()
                    .map(|f| f == prefix || f.starts_with(&prefix_dot))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Returns the first error, if any.
    pub fn first(&self) -> Option<&ValidationError> {
        self.errors.first()
    }

    /// Converts to a Result, returning Ok(()) if no errors.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn into_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }

    /// Merges another ValidationErrors into this one.
    pub fn merge(&mut self, other: ValidationErrors) {
        self.errors.extend(other.errors);
    }

    /// Merges errors from another source, prefixing all field paths.
    pub fn merge_with_prefix(&mut self, prefix: &str, other: ValidationErrors) {
        for error in other.errors {
            self.errors.push(error.with_prefix(prefix));
        }
    }
}

impl Error for ValidationErrors {
    fn error_code(&self) -> &'static str {
        "VALIDATION_ERROR"
    }

    fn display_message(&self) -> Cow<'_, str> {
        Cow::Borrowed("Validation failed")
    }

    fn http_status_code(&self) -> u16 {
        400
    }

    fn field_errors(&self) -> Vec<ValidationError> {
        self.errors.clone()
    }
}

// Unconditional: the crate's `Error` trait requires `core::error::Error`, also without `std`.
impl core::error::Error for ValidationErrors {}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        for error in &self.errors {
            writeln!(f, "  - {}", error)?;
        }
        Ok(())
    }
}

impl From<ValidationError> for ValidationErrors {
    fn from(error: ValidationError) -> Self {
        Self::single(error)
    }
}

impl IntoIterator for ValidationErrors {
    type Item = ValidationError;
    type IntoIter = <Vec<ValidationError> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<'a> IntoIterator for &'a ValidationErrors {
    type Item = &'a ValidationError;
    type IntoIter = core::slice::Iter<'a, ValidationError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.iter()
    }
}

impl Extend<ValidationError> for ValidationErrors {
    fn extend<T: IntoIterator<Item = ValidationError>>(&mut self, iter: T) {
        self.errors.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_every_error_it_holds_as_its_field_errors() {
        let mut errors = ValidationErrors::new();
        errors.add(ValidationError::required("email"));
        errors.add(ValidationError::too_short("name", 3, 1));

        assert_eq!(Error::field_errors(&errors), errors.errors().to_vec());
    }

    #[test]
    fn an_empty_collection_answers_no_field_errors() {
        assert!(Error::field_errors(&ValidationErrors::new()).is_empty());
    }

    #[test]
    fn test_validation_errors_collection() {
        let mut errors = ValidationErrors::new();
        assert!(errors.is_empty());

        errors.add(ValidationError::required("email"));
        errors.add(ValidationError::too_short("name", 3, 1));

        assert_eq!(errors.len(), 2);
        assert_eq!(errors.errors_for_field("email").len(), 1);
    }

    #[test]
    fn test_validation_errors_into_result() {
        let errors = ValidationErrors::new();
        assert!(errors.into_result().is_ok());

        let mut errors = ValidationErrors::new();
        errors.add(ValidationError::required("email"));
        assert!(errors.into_result().is_err());
    }

    #[test]
    fn test_errors_for_prefix() {
        let mut errors = ValidationErrors::new();
        errors.add(ValidationError::required("user.email"));
        errors.add(ValidationError::required("user.name"));
        errors.add(ValidationError::required("other"));

        let user_errors = errors.errors_for_prefix("user");
        assert_eq!(user_errors.len(), 2);
    }
}
