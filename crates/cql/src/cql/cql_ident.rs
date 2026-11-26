//! The [`CqlIdent`] validated identifier.

use std::fmt;

use crate::CqlResult;
use crate::ident::validate_cql_identifier;

/// A keyspace, table or column name that has passed the fail-closed identifier
/// guard and may therefore be interpolated into statement text.
///
/// A segment beginning with `_` is rendered double-quoted, as CQL rejects it bare; nothing else
/// is quoted.
///
/// # Example
///
/// ```rust
/// # extern crate fundamental_cql;
/// use fundamental_cql::CqlIdent;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let table = CqlIdent::qualified("layer0", "_fundamental_migrations")?;
/// assert_eq!(table.as_cql(), r#"layer0."_fundamental_migrations""#);
///
/// assert!(CqlIdent::new("name; DROP TABLE t; --").is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CqlIdent(String);

impl CqlIdent {
    /// Validates a single unqualified name.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier)
    /// when `name` is not a plain identifier.
    pub fn new(name: &str) -> CqlResult<Self> {
        validate_cql_identifier(name, false)?;
        Ok(Self(render_segment(name)))
    }

    /// Validates a `keyspace.name` pair, rendering it keyspace-qualified.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier)
    /// when either segment is not a plain identifier.
    pub fn qualified(keyspace: &str, name: &str) -> CqlResult<Self> {
        validate_cql_identifier(keyspace, false)?;
        validate_cql_identifier(name, false)?;
        Ok(Self(format!(
            "{}.{}",
            render_segment(keyspace),
            render_segment(name)
        )))
    }

    /// The identifier as it appears in statement text.
    #[must_use]
    pub fn as_cql(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CqlIdent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Renders one validated segment, double-quoting only what CQL refuses bare.
///
/// Quoting a name created bare would address a different table: quoted identifiers keep case.
fn render_segment(segment: &str) -> String {
    if segment.starts_with('_') {
        format!("\"{segment}\"")
    } else {
        segment.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CqlError;
    use fundamental_core::Error;

    #[test]
    fn a_plain_name_renders_bare() {
        let ident = CqlIdent::new("vault_rows").expect("a plain name is valid");

        assert_eq!(ident.as_cql(), "vault_rows");
    }

    #[test]
    fn a_leading_underscore_is_quoted() {
        let ident = CqlIdent::new("_fundamental_migrations").expect("valid, if unusual");

        assert_eq!(ident.as_cql(), "\"_fundamental_migrations\"");
    }

    #[test]
    fn a_qualified_name_quotes_only_the_segment_that_needs_it() {
        let ident = CqlIdent::qualified("layer0", "_fundamental_migrations").expect("valid");

        assert_eq!(ident.as_cql(), "layer0.\"_fundamental_migrations\"");
    }

    #[test]
    fn an_injection_payload_is_refused_with_the_specific_error() {
        let err = CqlIdent::new("t; DROP KEYSPACE layer0; --").expect_err("must be refused");

        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
        assert_eq!(err.error_code(), "CQL_012");
    }

    #[test]
    fn a_qualified_name_guards_the_keyspace_too() {
        let err = CqlIdent::qualified("ks; DROP KEYSPACE other; --", "vault_rows")
            .expect_err("must be refused");

        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
    }

    #[test]
    fn an_empty_name_is_refused() {
        assert!(matches!(
            CqlIdent::new(""),
            Err(CqlError::InvalidIdentifier(_))
        ));
    }
}
