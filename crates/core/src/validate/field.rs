//! Chainable per-field validation against a [`ValidationContext`].

use super::validators::*;
use super::{ParamValue, ValidationContext, ValidationError};

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Validator for a single field.
#[derive(Debug)]
pub struct FieldValidator<'a> {
    /// The validation context that collects any errors produced for this field.
    pub ctx: &'a mut ValidationContext,
    /// The name of the field being validated.
    pub field: &'a str,
}

impl<'a> FieldValidator<'a> {
    /// Validates that the string is not empty.
    pub fn required(self, value: &str) -> Self {
        if let Err(e) = validate_required(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that an Option is Some.
    pub fn required_opt<T>(self, value: &Option<T>) -> Self {
        if let Err(e) = validate_required_opt(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates string length.
    pub fn length(self, value: &str, min: usize, max: usize) -> Self {
        if let Err(e) = validate_length(self.field, value, min, max) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates minimum string length.
    pub fn min_length(self, value: &str, min: usize) -> Self {
        if let Err(e) = validate_min_length(self.field, value, min) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates maximum string length.
    pub fn max_length(self, value: &str, max: usize) -> Self {
        if let Err(e) = validate_max_length(self.field, value, max) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that a value is within a range.
    pub fn range<T: PartialOrd + Copy + Into<ParamValue>>(self, value: T, min: T, max: T) -> Self {
        if let Err(e) = validate_range(self.field, value, min, max) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates minimum numeric value.
    pub fn min<T: PartialOrd + Copy + Into<ParamValue>>(self, value: T, min: T) -> Self {
        if let Err(e) = validate_min(self.field, value, min) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates maximum numeric value.
    pub fn max<T: PartialOrd + Copy + Into<ParamValue>>(self, value: T, max: T) -> Self {
        if let Err(e) = validate_max(self.field, value, max) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates email format (basic check).
    pub fn email(self, value: &str) -> Self {
        if let Err(e) = validate_email(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates URL format (basic check).
    pub fn url(self, value: &str) -> Self {
        if let Err(e) = validate_url(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates UUID format.
    pub fn uuid(self, value: &str) -> Self {
        if let Err(e) = validate_uuid(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that the value matches a pattern.
    #[cfg(feature = "std")]
    pub fn pattern(self, value: &str, pattern: &regex::Regex) -> Self {
        if let Err(e) = validate_pattern(self.field, value, pattern) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that the value matches a pattern string.
    /// The pattern is compiled on each call; prefer `pattern()` in hot paths.
    #[cfg(feature = "std")]
    pub fn pattern_str(self, value: &str, pattern: &str) -> Self {
        if let Err(e) = validate_pattern_str(self.field, value, pattern) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that the value is one of the allowed values.
    pub fn one_of(self, value: &str, allowed: &[&str]) -> Self {
        if let Err(e) = validate_one_of(self.field, value, allowed) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates collection size.
    pub fn size<T>(self, value: &[T], min: usize, max: usize) -> Self {
        if let Err(e) = validate_size(self.field, value, min, max) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates with a custom function.
    pub fn custom<F>(self, value: &str, f: F) -> Self
    where
        F: FnOnce(&str) -> Option<String>,
    {
        if let Some(message) = f(value) {
            self.ctx
                .errors
                .add(ValidationError::invalid_value(self.field, message));
        }
        self
    }

    /// Validates with a custom function that takes any value.
    pub fn custom_value<T, F>(self, value: &T, f: F) -> Self
    where
        F: FnOnce(&T) -> Option<String>,
    {
        if let Some(message) = f(value) {
            self.ctx
                .errors
                .add(ValidationError::invalid_value(self.field, message));
        }
        self
    }

    /// Validates alphanumeric characters only.
    pub fn alphanumeric(self, value: &str) -> Self {
        if let Err(e) = validate_alphanumeric(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that the value contains only ASCII characters.
    pub fn ascii(self, value: &str) -> Self {
        if let Err(e) = validate_ascii(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Validates that the value is a valid identifier (alphanumeric + underscore, starts with letter).
    pub fn identifier(self, value: &str) -> Self {
        if let Err(e) = validate_identifier(self.field, value) {
            self.ctx.errors.add(e);
        }
        self
    }

    /// Conditionally apply validation only if a condition is true.
    pub fn when<F>(self, condition: bool, f: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if condition { f(self) } else { self }
    }
}
