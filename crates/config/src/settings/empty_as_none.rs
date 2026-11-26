use serde::{Deserialize, Deserializer};

/// Deserializes an optional string, reading an empty one (an unset `${VAR:-}`) as absent.
///
/// ```
/// # use serde::Deserialize;
/// #[derive(Deserialize)]
/// #[serde(default)]
/// struct Settings {
///     #[serde(deserialize_with = "fundamental_config::settings::empty_as_none")]
///     tls_ca_cert_path: Option<String>,
/// }
///
/// # fn main() -> Result<(), toml::de::Error> {
/// let settings: Settings = toml::from_str(r#"tls_ca_cert_path = """#)?;
/// assert!(settings.tls_ca_cert_path.is_none());
/// # Ok(())
/// # }
/// # impl Default for Settings {
/// #     fn default() -> Self { Self { tls_ca_cert_path: None } }
/// # }
/// ```
///
/// # Errors
/// Whatever `deserializer` reports for a value that is neither a string nor null.
pub fn empty_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    struct Fixture {
        #[serde(deserialize_with = "super::empty_as_none")]
        path: Option<String>,
    }

    fn parse(toml: &str) -> Fixture {
        toml::from_str(toml).expect("the fixture parses")
    }

    #[test]
    fn an_empty_value_is_absent() {
        assert!(parse(r#"path = """#).path.is_none());
    }

    #[test]
    fn a_real_value_survives() {
        assert_eq!(
            parse(r#"path = "/etc/ssl/ca.pem""#).path.as_deref(),
            Some("/etc/ssl/ca.pem")
        );
    }

    #[test]
    fn an_absent_key_stays_absent() {
        assert!(parse("").path.is_none());
    }
}
