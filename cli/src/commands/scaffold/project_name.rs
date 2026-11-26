//! Validation of project and app names as Rust crate names ([`ProjectName`]).

use fundamental::Validate;
use fundamental::core::validate::{ValidationErrors, ValidationResult};

use crate::CliError;

#[derive(Validate)]
pub(crate) struct ProjectName {
    #[validate(custom = "validate_project_name")]
    value: String,
}

impl ProjectName {
    pub(crate) fn new<S: Into<String>>(name: S) -> Self {
        Self { value: name.into() }
    }
}

fn validate_project_name(name: &str) -> ValidationResult<()> {
    let mut errs = ValidationErrors::new();

    if name.is_empty() {
        errs.add(CliError::InvalidProjectName {
            name: name.to_string(),
            reason: "name cannot be empty".to_string(),
        });
    }

    let first_char = name.chars().next().unwrap();
    if !first_char.is_ascii_alphabetic() && first_char != '_' {
        errs.add(CliError::InvalidProjectName {
            name: name.to_string(),
            reason: "name must start with a letter or underscore".to_string(),
        });
    }

    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
            errs.add(CliError::InvalidProjectName {
                name: name.to_string(),
                reason: format!(
                    "invalid character '{}': only letters, numbers, underscores, and hyphens are allowed",
                    c
                ),
            });
        }
    }

    let reserved = ["test", "core", "std", "alloc", "proc_macro"];
    if reserved.contains(&name) {
        errs.add(CliError::InvalidProjectName {
            name: name.to_string(),
            reason: "this name is reserved".to_string(),
        });
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    Ok(())
}
