//! [`ShardKind`] — how a SQL key column's values are spread across tablets.

/// How a `#[model(id, shard = …)]` column distributes.
///
/// Left undeclared, YugabyteDB hashes the leading key column silently, which is wrong for a near-constant
/// residency column and for a column read by range.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ShardKind {
    /// Hashed: point lookups land on one tablet, an ordered scan cannot use the key; only a key's leading element.
    Hash,
    /// Stored ascending. A ranged or ordered read is a scan.
    Asc,
    /// Stored descending — the newest-first read.
    Desc,
}

impl ShardKind {
    /// The token the engine's DDL spells this with.
    #[must_use]
    pub const fn as_sql(&self) -> &'static str {
        match self {
            Self::Hash => "HASH",
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }

    /// Parses `hash`, `asc` or `desc`, in any case.
    #[must_use]
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "hash" => Some(Self::Hash),
            "asc" | "ascending" => Some(Self::Asc),
            "desc" | "descending" => Some(Self::Desc),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ShardKind;

    #[test]
    fn every_spelling_an_author_reaches_for_is_accepted() {
        assert_eq!(ShardKind::parse_str("HASH"), Some(ShardKind::Hash));
        assert_eq!(ShardKind::parse_str("Asc"), Some(ShardKind::Asc));
        assert_eq!(ShardKind::parse_str("descending"), Some(ShardKind::Desc));
        assert_eq!(ShardKind::parse_str("random"), None);
    }

    #[test]
    fn it_renders_the_token_the_engine_reads_back() {
        assert_eq!(ShardKind::Hash.as_sql(), "HASH");
        assert_eq!(ShardKind::Desc.as_sql(), "DESC");
    }
}
