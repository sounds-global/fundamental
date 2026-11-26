use super::{SecretBaseDir, expand_env_vars};
use crate::{ConfigError, ConfigResult};

/// Recursively expands environment variables in a TOML value; `field_path` locates failures.
///
/// An array element that interpolated to nothing is dropped; a table key keeps its empty value.
///
/// # Errors
/// [`ConfigError::EnvVarExpansion`] carrying `field_path` for an unset variable without a default.
pub fn expand_env_vars_in_value(
    value: &mut toml::Value,
    field_path: &str,
    base: &SecretBaseDir,
) -> ConfigResult<()> {
    match value {
        toml::Value::String(s) => {
            if s.contains("${") {
                *s = expand_env_vars(s, base).map_err(|e| match e {
                    ConfigError::EnvVarNotFound { var } => ConfigError::EnvVarExpansion {
                        field: field_path.to_string(),
                        var,
                    },
                    other => other,
                })?;
            }
        }
        toml::Value::Array(arr) => {
            let mut vacated = Vec::new();
            for (i, item) in arr.iter_mut().enumerate() {
                let interpolated = matches!(item, toml::Value::String(s) if s.contains("${"));
                expand_env_vars_in_value(item, &format!("{field_path}[{i}]"), base)?;
                if interpolated && matches!(item, toml::Value::String(s) if s.is_empty()) {
                    vacated.push(i);
                }
            }
            for i in vacated.into_iter().rev() {
                arr.remove(i);
            }
        }
        toml::Value::Table(table) => {
            for (key, val) in table.iter_mut() {
                let path = if field_path.is_empty() {
                    key.clone()
                } else {
                    format!("{field_path}.{key}")
                };
                expand_env_vars_in_value(val, &path, base)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(value: &str) -> toml::Value {
        let mut parsed: toml::Value = toml::from_str(value).expect("the fixture parses");
        expand_env_vars_in_value(&mut parsed, "", &SecretBaseDir::default())
            .expect("the fixture expands");
        parsed
    }

    #[test]
    fn an_element_that_resolved_to_nothing_leaves_the_array() {
        let expanded = expand(r#"keys = ["${DEFINITELY_UNSET_VKEY:-}"]"#);
        assert_eq!(
            expanded["keys"].as_array().expect("an array").len(),
            0,
            "no keys, rather than one unparseable empty key"
        );
    }

    #[test]
    fn a_deliberate_empty_literal_survives() {
        let expanded = expand(r#"keys = ["", "a"]"#);
        assert_eq!(expanded["keys"].as_array().expect("an array").len(), 2);
    }

    #[test]
    fn only_the_vacated_elements_leave() {
        let expanded = expand(r#"hosts = ["a", "${DEFINITELY_UNSET_HOST:-}", "b"]"#);
        let hosts = expanded["hosts"].as_array().expect("an array");
        assert_eq!(hosts, &[toml::Value::from("a"), toml::Value::from("b")]);
    }

    #[test]
    fn a_resolved_default_is_kept() {
        let expanded = expand(r#"hosts = ["${DEFINITELY_UNSET_HOST:-127.0.0.1:9042}"]"#);
        assert_eq!(
            expanded["hosts"].as_array().expect("an array"),
            &[toml::Value::from("127.0.0.1:9042")]
        );
    }

    #[test]
    fn a_table_key_that_resolved_to_nothing_is_kept() {
        let expanded = expand(r#"signing_key = "${DEFINITELY_UNSET_SIGNING_KEY:-}""#);
        assert_eq!(expanded["signing_key"].as_str(), Some(""));
    }
}
