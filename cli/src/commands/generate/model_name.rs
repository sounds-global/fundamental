//! Validation and case-conversion of a user-supplied model name.

use crate::CliError;

/// A validated `PascalCase` model name, with the `snake_case` form naming table, module and migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelName {
    name: String,
    snake: String,
}

impl ModelName {
    /// Parse and validate a model name.
    ///
    /// # Errors
    /// [`CliError::InvalidModelName`] unless it is an ASCII letter then letters, digits and underscores.
    pub(crate) fn parse(name: &str) -> Result<Self, CliError> {
        if name.is_empty() {
            return Err(CliError::InvalidModelName {
                name: name.to_string(),
                reason: "name cannot be empty".to_string(),
            });
        }

        let first = name.chars().next().expect("name is non-empty");
        if !first.is_ascii_alphabetic() {
            return Err(CliError::InvalidModelName {
                name: name.to_string(),
                reason: "name must start with a letter".to_string(),
            });
        }

        if let Some(bad) = name
            .chars()
            .find(|c| !c.is_ascii_alphanumeric() && *c != '_')
        {
            return Err(CliError::InvalidModelName {
                name: name.to_string(),
                reason: format!(
                    "invalid character '{bad}': only letters, digits, and underscores are allowed"
                ),
            });
        }

        Ok(Self {
            snake: to_snake_case(name),
            name: name.to_string(),
        })
    }

    pub(crate) fn type_name(&self) -> &str {
        &self.name
    }

    pub(crate) fn snake(&self) -> &str {
        &self.snake
    }
}

/// A run of capitals is one acronym (`HTTPServer` → `http_server`); underscores are not doubled.
fn to_snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);

    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            out.push('_');
            continue;
        }
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase();
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_is_accepted_and_snaked() {
        let m = ModelName::parse("BlogPost").unwrap();
        assert_eq!(m.type_name(), "BlogPost");
        assert_eq!(m.snake(), "blog_post");
    }

    #[test]
    fn single_word_lowercases() {
        assert_eq!(ModelName::parse("Post").unwrap().snake(), "post");
    }

    #[test]
    fn acronym_runs_stay_together() {
        assert_eq!(
            ModelName::parse("HTTPServer").unwrap().snake(),
            "http_server"
        );
        assert_eq!(ModelName::parse("APIKey").unwrap().snake(), "api_key");
    }

    #[test]
    fn existing_underscores_are_preserved() {
        assert_eq!(ModelName::parse("Blog_Post").unwrap().snake(), "blog_post");
    }

    #[test]
    fn empty_name_is_rejected() {
        let err = ModelName::parse("").expect_err("empty");
        assert!(matches!(err, CliError::InvalidModelName { .. }));
    }

    #[test]
    fn a_leading_digit_is_rejected() {
        let err = ModelName::parse("2Cool").expect_err("leading digit");
        assert!(matches!(err, CliError::InvalidModelName { .. }));
        assert!(err.to_string().contains("start with a letter"));
    }

    #[test]
    fn punctuation_is_rejected() {
        let err = ModelName::parse("Bad-Name").expect_err("hyphen");
        assert!(matches!(err, CliError::InvalidModelName { .. }));
    }
}
