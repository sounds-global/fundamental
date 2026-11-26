use std::fmt;

/// The environment variable an origin reaches an app under: `[A-Z][A-Z0-9_]*`, `WWW_ORIGIN`.
///
/// Never `FUNDAMENTAL_…`, which overrides configuration, nor `FUN_LOCAL_…`, which holds the instance's own facts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginVar(String);

impl OriginVar {
    /// Parses a variable name, or says why it is not one.
    ///
    /// # Errors
    ///
    /// A sentence naming what is wrong with `text`, for [`LocalError::OriginsInvalid`].
    ///
    /// [`LocalError::OriginsInvalid`]: crate::LocalError::OriginsInvalid
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut bytes = text.bytes();
        let shaped = bytes.next().is_some_and(|byte| byte.is_ascii_uppercase())
            && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if !shaped {
            return Err(format!(
                "`{text}` is not an environment variable name: upper-case letters, digits and `_`, starting with a letter"
            ));
        }
        for reserved in ["FUNDAMENTAL_", "FUN_LOCAL_"] {
            if text.starts_with(reserved) {
                return Err(format!(
                    "`{text}` is in the `{reserved}` namespace, which is not an origin's to use"
                ));
            }
        }
        Ok(Self(text.to_owned()))
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OriginVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::OriginVar;

    #[test]
    fn an_origin_variable_is_upper_snake_case() {
        assert_eq!(
            OriginVar::parse("WWW_ORIGIN").map(|var| var.to_string()),
            Ok("WWW_ORIGIN".to_owned())
        );
        for refused in ["", "www", "1WWW", "WWW-ORIGIN", "WWW ORIGIN", "Www"] {
            assert!(OriginVar::parse(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_framework_namespaces_are_refused() {
        for reserved in ["FUNDAMENTAL_APPS__WWW__WEB__HOST", "FUN_LOCAL_ADDRESS"] {
            let refused = OriginVar::parse(reserved).expect_err("reserved");
            assert!(refused.contains("namespace"), "{refused}");
        }
    }
}
