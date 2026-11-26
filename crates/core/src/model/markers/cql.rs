//! The [`CqlDb`] database marker.

use super::Database;
use crate::Sealed;

/// Marker type for YugabyteDB's YCQL API, the Cassandra-compatible wide-column backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CqlDb;

impl Sealed for CqlDb {}

impl Database for CqlDb {
    const NAME: &'static str = "YugabyteDB YCQL";
    const SUPPORTS_TRANSACTIONS: bool = false; // The framework issues no YCQL transactions
    const SUPPORTS_JOINS: bool = false;
    const SUPPORTS_SECONDARY_INDEXES: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // The asserts pin associated consts, so they are constant by design.
    #[allow(clippy::assertions_on_constants)]
    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    fn test_cql_properties() {
        assert_eq!(CqlDb::NAME, "YugabyteDB YCQL");
        assert!(!CqlDb::SUPPORTS_TRANSACTIONS);
        assert!(!CqlDb::SUPPORTS_JOINS);
    }
}
