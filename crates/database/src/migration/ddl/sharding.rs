//! [`Sharding`]: how a key column's values are distributed across tablets.

use core::fmt;

/// How the engine distributes the values of a key element.
///
/// The engine picks one whether or not the author does, so a key always declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sharding {
    /// The values are hashed; only the leading element of a key may be.
    Hash,
    /// The values are stored in ascending order.
    Asc,
    /// The values are stored in descending order.
    Desc,
}

impl Sharding {
    /// Recognises `HASH`, `ASC` or `DESC`, in any case.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_uppercase().as_str() {
            "HASH" => Some(Self::Hash),
            "ASC" => Some(Self::Asc),
            "DESC" => Some(Self::Desc),
            _ => None,
        }
    }

    /// Whether this element distributes by hash rather than by order.
    #[must_use]
    pub fn is_hash(self) -> bool {
        matches!(self, Self::Hash)
    }
}

impl fmt::Display for Sharding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hash => "HASH",
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Sharding;

    #[test]
    fn every_spelling_the_engine_accepts_is_recognised() {
        assert_eq!(Sharding::parse("hash"), Some(Sharding::Hash));
        assert_eq!(Sharding::parse("Asc"), Some(Sharding::Asc));
        assert_eq!(Sharding::parse("DESC"), Some(Sharding::Desc));
    }

    #[test]
    fn a_column_name_is_not_a_sharding_token() {
        assert_eq!(Sharding::parse("zone"), None);
        assert_eq!(Sharding::parse(""), None);
    }

    #[test]
    fn only_hash_distributes_by_hash() {
        assert!(Sharding::Hash.is_hash());
        assert!(!Sharding::Asc.is_hash());
        assert!(!Sharding::Desc.is_hash());
    }
}
