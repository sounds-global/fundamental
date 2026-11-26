//! [`DatabaseType`]: the database backend schema metadata applies to.

/// Database type for schema generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseType {
    /// YugabyteDB YSQL (PostgreSQL-compatible SQL).
    SqlDb,
    /// YugabyteDB YCQL (Cassandra-compatible CQL).
    CqlDb,
    /// SQLite (for offline/testing).
    OfflineDb,
}
