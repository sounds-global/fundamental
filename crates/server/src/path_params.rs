use std::fmt;
use std::str::FromStr;

use crate::{ServerError, ServerResult};

/// The path parameters captured when a request path matched a route template.
#[derive(Debug, Clone, Default)]
pub struct PathParams {
    params: Vec<(String, String)>,
}

impl PathParams {
    /// Creates an empty set of path parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the raw value captured for `name`, if the route captured it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Returns the raw value captured for `name`.
    ///
    /// # Errors
    /// Returns [`ServerError::MissingPathParam`] if the route did not capture a
    /// parameter named `name`.
    pub fn require(&self, name: &str) -> ServerResult<&str> {
        self.get(name).ok_or_else(|| ServerError::MissingPathParam {
            name: name.to_owned(),
        })
    }

    /// Parses the value captured for `name` into `T`.
    ///
    /// # Errors
    /// - [`ServerError::MissingPathParam`] if the route did not capture `name`.
    /// - [`ServerError::InvalidPathParam`] if the captured value cannot be
    ///   parsed into `T`.
    pub fn parse<T>(&self, name: &str) -> ServerResult<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        let raw = self.require(name)?;
        raw.parse::<T>().map_err(|e| ServerError::InvalidPathParam {
            name: name.to_owned(),
            message: e.to_string(),
        })
    }

    /// Iterates the captured `(name, value)` pairs in capture order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.params
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns the number of captured parameters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.params.len()
    }

    /// Returns whether no parameters were captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
}

impl FromIterator<(String, String)> for PathParams {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            params: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error as _;

    fn params() -> PathParams {
        [
            ("id".to_owned(), "42".to_owned()),
            ("slug".to_owned(), "hello".to_owned()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn get_returns_captured_value() {
        let p = params();
        assert_eq!(p.get("id"), Some("42"));
        assert_eq!(p.get("slug"), Some("hello"));
        assert_eq!(p.get("missing"), None);
        assert_eq!(p.len(), 2);
        assert!(!p.is_empty());
    }

    #[test]
    fn empty_params_report_empty() {
        let p = PathParams::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn parse_converts_to_the_requested_type() {
        let p = params();
        let id: u64 = p.parse("id").expect("parses a number");
        assert_eq!(id, 42);
    }

    #[test]
    fn require_missing_param_is_a_programmer_error() {
        let p = params();
        let err = p.require("nope").expect_err("missing param");
        assert_eq!(err.error_code(), "API_005");
        assert_eq!(err.http_status_code(), 500);
        assert!(matches!(err, ServerError::MissingPathParam { .. }));
    }

    #[test]
    fn parse_missing_param_reports_missing_not_invalid() {
        let p = params();
        let err = p.parse::<u64>("nope").expect_err("missing param");
        assert_eq!(err.error_code(), "API_005");
    }

    #[test]
    fn parse_malformed_value_is_a_bad_request() {
        let p = params();
        let err = p.parse::<u64>("slug").expect_err("not a number");
        assert_eq!(err.error_code(), "API_006");
        assert_eq!(err.http_status_code(), 400);
        match err {
            ServerError::InvalidPathParam { name, .. } => assert_eq!(name, "slug"),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
