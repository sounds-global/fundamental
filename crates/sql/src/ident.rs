//! SQL identifier validation, which fails closed, for identifiers interpolated into query text.

use crate::SqlError;

/// Whether `s` is one safe SQL identifier: non-empty, at most 63 chars, `[A-Za-z_][A-Za-z0-9_]*`.
pub(crate) fn is_valid_identifier(s: &str) -> bool {
    fundamental_db::ident::is_valid_identifier(s)
}

/// Validates a dot-qualified identifier interpolated into SQL, and `*` when `allow_star` is set.
pub(crate) fn validate_identifier(raw: &str, allow_star: bool) -> Result<(), SqlError> {
    fundamental_db::ident::validate_identifier(raw, allow_star)
        .map_err(|e| SqlError::query(e.to_string()))
}

/// Validates an `ON CONFLICT` target; empty is refused, as it would emit no clause and turn an idempotent insert into a plain one.
pub(crate) fn validate_conflict_target(columns: &[&str]) -> Result<(), SqlError> {
    if columns.is_empty() {
        return Err(SqlError::query(
            "an ON CONFLICT target needs at least one column",
        ));
    }
    for column in columns {
        validate_identifier(column, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_conflict_target_may_be_composite_and_every_column_is_checked() {
        assert!(validate_conflict_target(&["region", "contact_email"]).is_ok());
        assert!(
            validate_conflict_target(&["region", "email); DROP TABLE staff_accounts --"]).is_err(),
            "a hostile column anywhere in the key must be refused"
        );
    }

    #[test]
    fn an_empty_conflict_target_is_refused() {
        assert!(validate_conflict_target(&[]).is_err());
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
}
