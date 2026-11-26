//! [`SchemaName`] — the schema a connection's statements are qualified with.

use core::fmt;

use crate::ident::{InvalidIdentifier, is_valid_identifier};

/// The schema every statement on a connection names its tables in.
///
/// Statements are still schema-qualified rather than left to `search_path`, because of
/// [CVE-2018-1058]. Validated on construction, so it can be interpolated into a statement.
///
/// [CVE-2018-1058]: https://www.postgresql.org/support/security/CVE-2018-1058
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SchemaName(String);

impl SchemaName {
    /// The schema a PostgreSQL-wire deployment gets when it says nothing.
    pub const DEFAULT: &'static str = "public";

    /// Wraps `name` after checking it is a single safe identifier.
    ///
    /// # Errors
    /// Returns [`InvalidIdentifier`] when `name` is not a single safe identifier.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidIdentifier> {
        let name = name.into();
        if is_valid_identifier(&name) {
            Ok(Self(name))
        } else {
            Err(InvalidIdentifier::new(name))
        }
    }

    /// A borrowable [`DEFAULT`](Self::DEFAULT), for a caller with nothing configured to ask.
    #[must_use]
    pub fn wire_default() -> &'static Self {
        static WIRE_DEFAULT: std::sync::LazyLock<SchemaName> =
            std::sync::LazyLock::new(SchemaName::default);
        &WIRE_DEFAULT
    }

    /// The schema name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `<schema>.<table>`, for a `table` the caller has already validated.
    #[must_use]
    pub fn qualify(&self, table: &str) -> String {
        format!("{}.{}", self.0, table)
    }
}

impl Default for SchemaName {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl fmt::Display for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaName;

    #[test]
    fn the_default_is_the_wire_s_own_default() {
        assert_eq!(SchemaName::default().as_str(), "public");
    }

    #[test]
    fn it_qualifies_a_table() {
        let schema = SchemaName::new("tenant_7").expect("valid");
        assert_eq!(schema.qualify("invoices"), "tenant_7.invoices");
    }

    #[test]
    fn a_name_that_could_escape_a_statement_cannot_be_constructed() {
        for hostile in [
            "public; DROP TABLE users; --",
            "public.users",
            "\"public\"",
            "",
            "1st",
            "with space",
        ] {
            assert!(SchemaName::new(hostile).is_err(), "must refuse {hostile:?}");
        }
    }
}
