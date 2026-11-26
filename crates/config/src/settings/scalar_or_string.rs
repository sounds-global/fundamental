use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// Deserializes a number or boolean that may arrive as a string, so `${VAR}` interpolation can reach it.
///
/// ```
/// # use serde::Deserialize;
/// #[derive(Deserialize)]
/// #[serde(default)]
/// struct Settings {
///     #[serde(deserialize_with = "fundamental_config::settings::scalar_or_string")]
///     port: u16,
///     #[serde(deserialize_with = "fundamental_config::settings::scalar_or_string")]
///     enabled: bool,
/// }
///
/// # fn main() -> Result<(), toml::de::Error> {
/// // Written by hand …
/// let literal: Settings = toml::from_str("port = 16000\nenabled = true")?;
/// // … or supplied by an expanded `${VAR:-…}`.
/// let expanded: Settings = toml::from_str(r#"port = "16000"
/// enabled = "true""#)?;
///
/// assert_eq!(literal.port, expanded.port);
/// assert_eq!(literal.enabled, expanded.enabled);
/// # Ok(())
/// # }
/// # impl Default for Settings {
/// #     fn default() -> Self { Self { port: 0, enabled: false } }
/// # }
/// ```
///
/// # Errors
/// A string that does not parse into `T`, or an empty one.
pub fn scalar_or_string<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    match ScalarOrString::<T>::deserialize(deserializer)? {
        ScalarOrString::Scalar(value) => Ok(value),
        ScalarOrString::Text(text) => parse(&text),
    }
}

/// [`scalar_or_string`] for a setting whose absence is meaningful: an empty string reads as `None`.
///
/// # Errors
/// As [`scalar_or_string`], for a non-empty string that does not parse.
pub fn optional_scalar_or_string<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + FromStr,
    <T as FromStr>::Err: Display,
{
    match Option::<ScalarOrString<T>>::deserialize(deserializer)? {
        None => Ok(None),
        Some(ScalarOrString::Scalar(value)) => Ok(Some(value)),
        Some(ScalarOrString::Text(text)) if text.trim().is_empty() => Ok(None),
        Some(ScalarOrString::Text(text)) => parse(&text).map(Some),
    }
}

/// `untagged` tries the native type first.
#[derive(Deserialize)]
#[serde(untagged)]
enum ScalarOrString<T> {
    Scalar(T),
    Text(String),
}

fn parse<T, E>(text: &str) -> Result<T, E>
where
    T: FromStr,
    <T as FromStr>::Err: Display,
    E: serde::de::Error,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(E::custom(format!(
            "expected a {}, but the value expanded to nothing — a `${{VAR:-}}` with \
             no default on a setting that has no absent value; give the interpolation \
             a default, as in `\"${{VAR:-16000}}\"`",
            std::any::type_name::<T>()
        )));
    }
    trimmed.parse().map_err(|error| {
        E::custom(format!(
            "expected a {}, got {text:?}: {error}",
            std::any::type_name::<T>()
        ))
    })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Fixture {
        #[serde(deserialize_with = "super::scalar_or_string")]
        port: u16,
        #[serde(deserialize_with = "super::scalar_or_string")]
        enabled: bool,
        #[serde(deserialize_with = "super::scalar_or_string")]
        ratio: f64,
    }

    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    struct Optional {
        #[serde(deserialize_with = "super::optional_scalar_or_string")]
        endpoint_port: Option<u16>,
    }

    #[test]
    fn a_native_value_parses_as_it_always_did() {
        let fixture: Fixture =
            toml::from_str("port = 16000\nenabled = true\nratio = 0.1").expect("native types");
        assert_eq!(fixture.port, 16000);
        assert!(fixture.enabled);
        assert!((fixture.ratio - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn an_expanded_string_parses_into_the_setting_s_type() {
        let fixture: Fixture = toml::from_str(
            r#"port = "16000"
               enabled = "true"
               ratio = "0.1""#,
        )
        .expect("expanded strings");
        assert_eq!(fixture.port, 16000);
        assert!(fixture.enabled);
        assert!((fixture.ratio - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn an_integer_satisfies_a_float_setting() {
        let fixture: Fixture =
            toml::from_str("port = 1\nenabled = false\nratio = 1").expect("integer for a float");
        assert!((fixture.ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_malformed_value_names_itself_and_fails() {
        let error = toml::from_str::<Fixture>(
            r#"port = "16OOO"
               enabled = "true"
               ratio = "1""#,
        )
        .expect_err("the port is not a number");
        let message = error.to_string();
        assert!(message.contains("16OOO"), "{message}");
        assert!(message.contains("u16"), "{message}");
    }

    #[test]
    fn an_empty_expansion_on_a_required_setting_explains_the_fix() {
        let error = toml::from_str::<Fixture>(
            r#"port = ""
               enabled = "true"
               ratio = "1""#,
        )
        .expect_err("an empty expansion cannot be a port");
        assert!(error.to_string().contains("expanded to nothing"));
    }

    #[test]
    fn a_port_survives_the_expander_and_arrives_as_a_number() {
        use crate::source::{SecretBaseDir, expand_env_vars_in_value};

        let mut document: toml::Value = toml::from_str(
            r#"port = "${DEFINITELY_UNSET_ADMIN_PORT:-16100}"
               enabled = "${DEFINITELY_UNSET_ADMIN_ENABLED:-true}"
               ratio = "${DEFINITELY_UNSET_SAMPLE_RATIO:-0.25}""#,
        )
        .expect("the document parses");
        expand_env_vars_in_value(&mut document, "", &SecretBaseDir::default())
            .expect("the document expands");

        let fixture: Fixture = document
            .try_into()
            .expect("the expanded document deserializes");
        assert_eq!(fixture.port, 16100);
        assert!(fixture.enabled);
        assert!((fixture.ratio - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_expansion_on_an_optional_setting_is_absent() {
        let optional: Optional = toml::from_str(r#"endpoint_port = """#).expect("empty is absent");
        assert!(optional.endpoint_port.is_none());

        let set: Optional = toml::from_str(r#"endpoint_port = "8080""#).expect("a real value");
        assert_eq!(set.endpoint_port, Some(8080));
    }
}
