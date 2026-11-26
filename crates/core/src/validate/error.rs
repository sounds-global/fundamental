//! A single validation failure and its typed parameters.

use core::fmt;
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::Cow,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

#[cfg(feature = "std")]
use std::{borrow::Cow, collections::BTreeMap};

use crate::i18n::{Arg, ArgValue};

/// A structured value attached to a [`ValidationError`] as a parameter.
///
/// Numbers stay typed so a message's CLDR plural selection works.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    /// A signed integer (lengths, counts, sizes, integer bounds).
    Int(i64),
    /// A floating-point number (real-valued range bounds).
    Float(f64),
    /// A string parameter (patterns, allowed-value lists, units).
    Str(String),
}

impl ParamValue {
    /// Borrows this parameter as a translation [`ArgValue`], preserving its numeric type.
    pub(crate) fn as_arg(&self) -> ArgValue<'_> {
        match self {
            ParamValue::Int(i) => ArgValue::Int(*i),
            ParamValue::Float(f) => ArgValue::Float(*f),
            ParamValue::Str(s) => ArgValue::Str(s.as_str()),
        }
    }
}

impl fmt::Display for ParamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamValue::Int(i) => write!(f, "{i}"),
            ParamValue::Float(x) => write!(f, "{x}"),
            ParamValue::Str(s) => f.write_str(s),
        }
    }
}

// Hand-written so `Float` compares by bit pattern, keeping `Eq` a true equivalence.
impl PartialEq for ParamValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ParamValue::Int(a), ParamValue::Int(b)) => a == b,
            (ParamValue::Float(a), ParamValue::Float(b)) => a.to_bits() == b.to_bits(),
            (ParamValue::Str(a), ParamValue::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for ParamValue {}

macro_rules! param_from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for ParamValue {
            fn from(v: $t) -> Self { ParamValue::Int(v as i64) }
        }
    )*};
}
param_from_int!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl From<f32> for ParamValue {
    fn from(v: f32) -> Self {
        ParamValue::Float(v as f64)
    }
}

impl From<f64> for ParamValue {
    fn from(v: f64) -> Self {
        ParamValue::Float(v)
    }
}

impl From<String> for ParamValue {
    fn from(v: String) -> Self {
        ParamValue::Str(v)
    }
}

impl From<&str> for ParamValue {
    fn from(v: &str) -> Self {
        ParamValue::Str(v.to_string())
    }
}

impl From<char> for ParamValue {
    fn from(v: char) -> Self {
        ParamValue::Str(v.to_string())
    }
}

impl From<bool> for ParamValue {
    fn from(v: bool) -> Self {
        ParamValue::Str(v.to_string())
    }
}

/// A single validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    /// The field path that failed validation (e.g., "user.address.city").
    pub field: Option<String>,

    /// A machine-readable error code.
    pub code: String,

    /// A human-readable error message.
    pub message: String,

    /// Developer-facing message for debugging (not intended for end users).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_message: Option<String>,

    /// Additional context for the error.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
}

impl ValidationError {
    /// Creates a new validation error.
    pub fn new(
        field: impl Into<Option<String>>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
            message: message.into(),
            developer_message: None,
            params: BTreeMap::new(),
        }
    }

    /// Creates a "required" validation error.
    pub fn required(field: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "required",
            format!("{} is required", field),
        )
    }

    /// Creates an "invalid format" validation error.
    pub fn invalid_format(field: impl Into<String>, details: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "invalid_format",
            format!("{}: {}", field, details.into()),
        )
    }

    /// Creates a "too short" validation error.
    pub fn too_short(field: impl Into<String>, min: usize, actual: usize) -> Self {
        let field = field.into();
        let mut error = Self::new(
            Some(field.clone()),
            "too_short",
            format!(
                "{} must be at least {} characters (got {})",
                field, min, actual
            ),
        );
        error.params.insert("min".into(), min.into());
        error.params.insert("actual".into(), actual.into());
        error
    }

    /// Creates a "too long" validation error.
    pub fn too_long(field: impl Into<String>, max: usize, actual: usize) -> Self {
        let field = field.into();
        let mut error = Self::new(
            Some(field.clone()),
            "too_long",
            format!(
                "{} must be at most {} characters (got {})",
                field, max, actual
            ),
        );
        error.params.insert("max".into(), max.into());
        error.params.insert("actual".into(), actual.into());
        error
    }

    /// Creates a "too small" validation error for numeric values.
    pub fn too_small(
        field: impl Into<String>,
        min: impl Into<ParamValue>,
        actual: impl Into<ParamValue>,
    ) -> Self {
        let field = field.into();
        let min = min.into();
        let actual = actual.into();
        let mut error = Self::new(
            Some(field.clone()),
            "too_small",
            format!("{} must be at least {} (got {})", field, min, actual),
        );
        error.params.insert("min".into(), min);
        error.params.insert("actual".into(), actual);
        error
    }

    /// Creates a "too large" validation error for numeric values.
    pub fn too_large(
        field: impl Into<String>,
        max: impl Into<ParamValue>,
        actual: impl Into<ParamValue>,
    ) -> Self {
        let field = field.into();
        let max = max.into();
        let actual = actual.into();
        let mut error = Self::new(
            Some(field.clone()),
            "too_large",
            format!("{} must be at most {} (got {})", field, max, actual),
        );
        error.params.insert("max".into(), max);
        error.params.insert("actual".into(), actual);
        error
    }

    /// Creates an "invalid value" validation error.
    pub fn invalid_value(field: impl Into<String>, message: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(Some(field), "invalid_value", message)
    }

    /// Creates a "not unique" validation error.
    pub fn not_unique(field: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "not_unique",
            format!("{} already exists", field),
        )
    }

    /// Creates an "invalid email" validation error.
    pub fn invalid_email(field: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "invalid_email",
            format!("{} must be a valid email address", field),
        )
    }

    /// Creates an "invalid url" validation error.
    pub fn invalid_url(field: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "invalid_url",
            format!("{} must be a valid URL", field),
        )
    }

    /// Creates an "invalid uuid" validation error.
    pub fn invalid_uuid(field: impl Into<String>) -> Self {
        let field = field.into();
        Self::new(
            Some(field.clone()),
            "invalid_uuid",
            format!("{} must be a valid UUID", field),
        )
    }

    /// Creates a "pattern mismatch" validation error.
    pub fn pattern_mismatch(field: impl Into<String>, pattern: impl Into<String>) -> Self {
        let field = field.into();
        let pattern = pattern.into();
        let mut error = Self::new(
            Some(field.clone()),
            "pattern_mismatch",
            format!("{} does not match the required pattern", field),
        );
        error.params.insert("pattern".into(), pattern.into());
        error
    }

    /// Creates a "not in list" validation error.
    pub fn not_in_list(field: impl Into<String>, allowed: &[&str]) -> Self {
        let field = field.into();
        let allowed_str = allowed.join(", ");
        let mut error = Self::new(
            Some(field.clone()),
            "not_in_list",
            format!("{} must be one of: {}", field, allowed_str),
        );
        error.params.insert("allowed".into(), allowed_str.into());
        error
    }

    /// Creates a custom validation error without a field.
    pub fn custom(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(None::<String>, code, message)
    }

    /// Builder to set developer message.
    #[must_use]
    pub fn with_developer_message(mut self, msg: impl Into<String>) -> Self {
        self.developer_message = Some(msg.into());
        self
    }

    /// Adds a parameter to this error.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<ParamValue>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Sets or replaces the field for this error.
    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Prefixes the field path (for nested validation).
    #[must_use]
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.field = Some(match self.field {
            Some(f) => format!("{}.{}", prefix, f),
            None => prefix.to_string(),
        });
        self
    }

    /// Returns the user-facing message, localized if a translator is installed.
    ///
    /// The `code` is the translation key (`validate.{code}`), its params and `field` the arguments.
    pub fn localized_message(&self) -> Cow<'_, str> {
        let key = format!("validate.{}", self.code);
        let mut args: Vec<Arg<'_>> = Vec::with_capacity(self.params.len() + 1);
        if let Some(field) = &self.field {
            args.push(("field", ArgValue::Str(field)));
        }
        for (name, value) in &self.params {
            args.push((name.as_str(), value.as_arg()));
        }
        crate::i18n::translate(&key, &args, || Cow::Borrowed(self.message.as_str()))
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error_required() {
        let err = ValidationError::required("email");
        assert_eq!(err.field, Some("email".into()));
        assert_eq!(err.code, "required");
        assert!(err.message.contains("email"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn localized_message_falls_back_to_source() {
        let err = ValidationError::too_short("password", 8, 3);
        assert_eq!(
            err.localized_message(),
            "password must be at least 8 characters (got 3)"
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn localized_message_routes_field_and_params() {
        use crate::i18n::{Arg, Translator, with};
        use std::sync::Arc;

        struct Es;
        impl Translator for Es {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
                let get = |n: &str| {
                    args.iter()
                        .find(|(k, _)| *k == n)
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_default()
                };
                (key == "validate.too_short").then(|| {
                    format!(
                        "{} debe tener al menos {} caracteres (tiene {})",
                        get("field"),
                        get("min"),
                        get("actual")
                    )
                })
            }
        }

        let err = ValidationError::too_short("password", 8, 3);
        let rendered = with(Arc::new(Es), || err.localized_message().into_owned());
        assert_eq!(
            rendered,
            "password debe tener al menos 8 caracteres (tiene 3)"
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn numeric_params_reach_translator_as_typed_ints() {
        use crate::i18n::{Arg, ArgValue, Translator, with};
        use std::sync::Arc;

        // Pluralizes on `min`, asserting it arrives as a typed integer.
        struct Plural;
        impl Translator for Plural {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn lookup(&self, key: &str, args: &[Arg<'_>]) -> Option<String> {
                if key != "validate.too_short" {
                    return None;
                }
                let min = args.iter().find(|(n, _)| *n == "min").map(|(_, v)| v)?;
                match min {
                    ArgValue::Int(1) => Some("must be at least 1 character".to_string()),
                    ArgValue::Int(n) => Some(format!("must be at least {n} characters")),
                    _ => panic!("min reached the translator as a non-integer: {min}"),
                }
            }
        }

        let one = ValidationError::too_short("password", 1, 0);
        let many = ValidationError::too_short("password", 8, 3);
        with(Arc::new(Plural), || {
            assert_eq!(one.localized_message(), "must be at least 1 character");
            assert_eq!(many.localized_message(), "must be at least 8 characters");
        });
    }

    #[test]
    fn params_serialize_as_typed_json() {
        // Integer params are JSON numbers, string params stay strings.
        let err = ValidationError::too_short("password", 8, 3)
            .with_param("unit", "chars")
            .with_param("ratio", 1.5_f64);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["params"]["min"], serde_json::json!(8));
        assert_eq!(json["params"]["actual"], serde_json::json!(3));
        assert_eq!(json["params"]["unit"], serde_json::json!("chars"));
        assert_eq!(json["params"]["ratio"], serde_json::json!(1.5));

        let back: ValidationError = serde_json::from_value(json).unwrap();
        assert_eq!(back, err);
        assert_eq!(back.params["min"], ParamValue::Int(8));
        assert_eq!(back.params["ratio"], ParamValue::Float(1.5));
    }
}
