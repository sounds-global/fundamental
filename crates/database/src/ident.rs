//! SQL/CQL identifier validation: the one guard for identifiers interpolated into query text.
//!
//! Identifiers cannot be bound as parameters, so any that reaches query text must pass
//! [`validate_identifier`], which fails closed.

use core::fmt;

use tracing::warn;

/// The PostgreSQL limit, also safe for CQL.
const MAX_IDENT_LEN: usize = 63;

/// Returns `true` if `s` is a single safe identifier: non-empty, `<= 63` chars,
/// ASCII `[A-Za-z_][A-Za-z0-9_]*`.
#[must_use]
pub fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_IDENT_LEN
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true)
}

/// Returns `true` if `s` is a [safe identifier](is_valid_identifier) with no
/// uppercase letter, so it can be quoted into a statement and keep its case.
#[must_use]
pub fn is_lowercase_identifier(s: &str) -> bool {
    is_valid_identifier(s) && !s.bytes().any(|byte| byte.is_ascii_uppercase())
}

/// Returns `true` if `s` is a stored name: `[a-z][a-z0-9._-]{0,63}`.
#[must_use]
pub fn is_stored_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    s.len() <= MAX_IDENT_LEN + 1
        && bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// Validates a possibly dot-qualified identifier that will be interpolated into a query;
/// with `allow_star`, a lone `*` is accepted too.
///
/// # Errors
///
/// Returns [`InvalidIdentifier`] if `raw` is empty, contains an empty segment,
/// or any segment is not a safe identifier.
pub fn validate_identifier(raw: &str, allow_star: bool) -> Result<(), InvalidIdentifier> {
    if allow_star && raw == "*" {
        return Ok(());
    }

    if !raw.is_empty() && raw.split('.').all(is_valid_identifier) {
        Ok(())
    } else {
        warn!(identifier = %raw, "rejected unsafe SQL identifier (fail-closed)");
        Err(InvalidIdentifier {
            identifier: raw.to_string(),
        })
    }
}

/// An identifier refused before reaching query text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidIdentifier {
    identifier: String,
}

impl InvalidIdentifier {
    /// Names `identifier` as refused, for a guard with a stricter rule of its own.
    #[must_use]
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }

    /// The rejected identifier, for diagnostics.
    #[must_use]
    pub fn identifier(&self) -> &str {
        &self.identifier
    }
}

impl fmt::Display for InvalidIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rejected unsafe SQL identifier {:?}: only [A-Za-z_][A-Za-z0-9_]* \
             (optionally dot-qualified) is allowed",
            self.identifier
        )
    }
}

impl std::error::Error for InvalidIdentifier {}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[test]
    fn accepts_plain_and_dotted_identifiers() {
        for ok in [
            "id",
            "user_name",
            "_x",
            "users.id",
            "public.users.name",
            "a1_b2",
        ] {
            assert!(validate_identifier(ok, false).is_ok(), "{ok} should pass");
        }
    }

    #[test]
    fn accepts_star_only_when_allowed() {
        assert!(validate_identifier("*", true).is_ok());
        assert!(validate_identifier("*", false).is_err());
    }

    #[test]
    fn rejects_injection_payloads() {
        for bad in [
            "x; DROP TABLE users; --",
            "a\" OR \"1\"=\"1",
            "a OR 1=1",
            "name = 1; DELETE FROM users",
            "1name",  // leading digit
            "na me",  // space
            "users.", // empty segment
            ".id",    // empty segment
            "col)",   // operator/paren
            "col--comment",
            "",
        ] {
            assert!(
                validate_identifier(bad, true).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_oversized_segment() {
        let too_long = "a".repeat(MAX_IDENT_LEN + 1);
        assert!(validate_identifier(&too_long, false).is_err());
        let at_limit = "a".repeat(MAX_IDENT_LEN);
        assert!(validate_identifier(&at_limit, false).is_ok());
    }

    #[traced_test]
    #[test]
    fn rejection_emits_fail_closed_warning() {
        let err = validate_identifier("x; DROP TABLE users; --", false);
        assert!(err.is_err(), "injection payload must be rejected");
        assert!(
            logs_contain("rejected unsafe SQL identifier (fail-closed)"),
            "a fail-closed rejection must be logged at the security boundary"
        );
    }

    #[test]
    fn error_exposes_rejected_identifier_and_message() {
        let err = validate_identifier("1bad", false).unwrap_err();
        assert_eq!(err.identifier(), "1bad");
        assert!(err.to_string().contains("rejected unsafe SQL identifier"));
        assert!(err.to_string().contains("1bad"));
    }

    #[test]
    fn a_lowercase_identifier_is_a_safe_one_without_capitals() {
        assert!(is_lowercase_identifier("app_role"));
        for name in ["App", "app role", "app\"; DROP", "", "1st"] {
            assert!(!is_lowercase_identifier(name), "{name:?}");
        }
    }

    #[test]
    fn a_stored_name_is_bounded_lowercase_and_never_a_path() {
        for name in ["day-zero", "hold.lapse", "production.v2", "a"] {
            assert!(is_stored_name(name), "{name:?}");
        }
        assert!(is_stored_name(&"a".repeat(64)));
        for refused in [
            "", "1st", "Day", "a b", "ü", "a/b", "../up", ".hidden", "_a",
        ] {
            assert!(!is_stored_name(refused), "{refused:?}");
        }
        assert!(!is_stored_name(&"a".repeat(65)));
    }
}
