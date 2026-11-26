//! A parsed model field: a `snake_case` name and a verbatim Rust type.

use crate::CliError;

/// One `name:type` field of a generated model: a `snake_case` name and a verbatim Rust type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    name: String,
    rust_type: String,
}

impl Field {
    /// Parse and validate a `name:type` field specification.
    ///
    /// # Errors
    /// [`CliError::InvalidField`] if it is not `name:type` with a `snake_case` name and a type.
    pub(crate) fn parse(spec: &str) -> Result<Self, CliError> {
        let (name, rust_type) = spec.split_once(':').ok_or_else(|| CliError::InvalidField {
            spec: spec.to_string(),
            reason: "expected `name:type`".to_string(),
        })?;
        let name = name.trim();
        let rust_type = rust_type.trim();

        if rust_type.is_empty() {
            return Err(CliError::InvalidField {
                spec: spec.to_string(),
                reason: "the type is empty".to_string(),
            });
        }
        Self::validate_name(spec, name)?;

        Ok(Self {
            name: name.to_string(),
            rust_type: rust_type.to_string(),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn rust_type(&self) -> &str {
        &self.rust_type
    }

    fn validate_name(spec: &str, name: &str) -> Result<(), CliError> {
        if name.is_empty() {
            return Err(CliError::InvalidField {
                spec: spec.to_string(),
                reason: "the field name is empty".to_string(),
            });
        }
        let first = name.chars().next().expect("name is non-empty");
        if !first.is_ascii_lowercase() && first != '_' {
            return Err(CliError::InvalidField {
                spec: spec.to_string(),
                reason: "the field name must start with a lowercase letter or underscore"
                    .to_string(),
            });
        }
        if let Some(bad) = name
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_'))
        {
            return Err(CliError::InvalidField {
                spec: spec.to_string(),
                reason: format!(
                    "invalid character '{bad}' in field name: only lowercase letters, digits, and underscores are allowed"
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_whitespace_around_the_parts() {
        let f = Field::parse(" views : i64 ").unwrap();
        assert_eq!(f.name(), "views");
        assert_eq!(f.rust_type(), "i64");
    }

    #[test]
    fn missing_colon_is_rejected() {
        let err = Field::parse("title").expect_err("no colon");
        assert!(matches!(err, CliError::InvalidField { .. }));
    }

    #[test]
    fn empty_type_is_rejected() {
        let err = Field::parse("title:").expect_err("empty type");
        assert!(matches!(err, CliError::InvalidField { .. }));
    }

    #[test]
    fn a_non_snake_case_name_is_rejected() {
        let err = Field::parse("Title:String").expect_err("uppercase name");
        assert!(matches!(err, CliError::InvalidField { .. }));
    }

    #[test]
    fn a_hyphen_in_the_name_is_rejected() {
        let err = Field::parse("my-field:String").expect_err("hyphen");
        assert!(matches!(err, CliError::InvalidField { .. }));
    }
}
