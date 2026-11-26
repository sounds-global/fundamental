//! The [`SqlDb`] database marker type and its [`Database`](super::Database) impl.

use super::Database;
use crate::Sealed;

/// Marker type for YugabyteDB's YSQL API, the PostgreSQL-compatible relational backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SqlDb;

impl Sealed for SqlDb {}

impl Database for SqlDb {
    const NAME: &'static str = "YugabyteDB YSQL";
    const SUPPORTS_TRANSACTIONS: bool = true;
    const SUPPORTS_JOINS: bool = true;
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
    fn test_sql_properties() {
        assert_eq!(SqlDb::NAME, "YugabyteDB YSQL");
        assert!(SqlDb::SUPPORTS_TRANSACTIONS);
        assert!(SqlDb::SUPPORTS_JOINS);
    }
}
