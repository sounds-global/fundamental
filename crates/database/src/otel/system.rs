//! The [`DbSystem`] enum, the `db.system` OpenTelemetry attribute.

/// Database system identifiers following OTel conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbSystem {
    /// YSQL (PostgreSQL-compatible)
    SqlDb,
    /// YCQL (Cassandra-compatible)
    CqlDb,
}

impl DbSystem {
    /// Returns the OpenTelemetry db.system attribute value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SqlDb => "postgresql",
            Self::CqlDb => "cassandra",
        }
    }
}

impl std::fmt::Display for DbSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
