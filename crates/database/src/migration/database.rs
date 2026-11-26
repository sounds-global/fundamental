//! Migration target databases.

use std::fmt;

/// Database type for migrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MigrationDatabase {
    /// YugabyteDB YSQL (PostgreSQL-compatible SQL).
    SqlDb,
    /// YugabyteDB YCQL (Cassandra-compatible CQL).
    CqlDb,
    /// SQLite, on the device (offline).
    OfflineDb,
}

impl fmt::Display for MigrationDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SqlDb => "YugabyteDB YSQL",
            Self::CqlDb => "YugabyteDB YCQL",
            Self::OfflineDb => "SQLite",
        })
    }
}

impl MigrationDatabase {
    /// Returns the directory name for migrations.
    pub fn directory_name(&self) -> &'static str {
        match self {
            Self::SqlDb => "sql",
            Self::CqlDb => "cql",
            Self::OfflineDb => "offline",
        }
    }
}
