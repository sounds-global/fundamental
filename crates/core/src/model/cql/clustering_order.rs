//! Sort order (ascending or descending) for YCQL clustering keys.

/// Clustering key order for YCQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClusteringOrder {
    #[default]
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

impl ClusteringOrder {
    /// Returns the CQL representation.
    pub const fn as_cql(&self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }

    /// Parses a string into a [`ClusteringOrder`].
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "asc" | "ascending" => Some(Self::Asc),
            "desc" | "descending" => Some(Self::Desc),
            _ => None,
        }
    }
}
