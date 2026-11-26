//! CQL identifier guard.

use fundamental_db::ident::validate_identifier;
use tracing::warn;

use crate::{CqlError, CqlResult};

/// Validates a caller-supplied CQL identifier, failing closed.
///
/// `allow_star` also accepts a lone `*`. A rejection is logged by its length only.
///
/// # Errors
///
/// Returns [`CqlError::InvalidIdentifier`] if `raw` is not a safe identifier.
pub(crate) fn validate_cql_identifier(raw: &str, allow_star: bool) -> CqlResult<()> {
    validate_identifier(raw, allow_star).map_err(|e| {
        warn!(
            identifier_len = raw.len(),
            "rejected unsafe CQL identifier before interpolation"
        );
        CqlError::invalid_identifier(e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_and_star() {
        assert!(validate_cql_identifier("user_id", false).is_ok());
        assert!(validate_cql_identifier("events.ts", false).is_ok());
        assert!(validate_cql_identifier("*", true).is_ok());
    }

    #[test]
    fn rejects_injection_with_invalid_identifier_error() {
        let err = validate_cql_identifier("hits = hits + 1; DROP TABLE t; --", false)
            .expect_err("must reject injection payload");
        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
    }

    #[test]
    fn rejects_star_when_not_allowed() {
        assert!(matches!(
            validate_cql_identifier("*", false),
            Err(CqlError::InvalidIdentifier(_))
        ));
    }
}
