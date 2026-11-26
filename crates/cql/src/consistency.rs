//! The [`Consistency`] level a CQL read or write runs at.

use scylla::statement::Consistency as CqlConsistency;

/// Consistency level for a CQL read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Consistency {
    /// Wait for one replica.
    One,

    /// Wait for two replicas.
    Two,

    /// Wait for three replicas.
    Three,

    /// Wait for a quorum (majority) of replicas.
    Quorum,

    /// Wait for all replicas.
    All,

    /// Wait for a quorum of replicas in the local datacenter.
    #[default]
    LocalQuorum,

    /// Wait for one replica in the local datacenter.
    LocalOne,

    /// Wait for at least one replica in each datacenter.
    EachQuorum,

    /// Return results from any available replica (may be stale).
    Any,
}

impl Consistency {
    /// Converts to the driver's consistency type.
    pub fn to_driver(self) -> CqlConsistency {
        match self {
            Self::One => CqlConsistency::One,
            Self::Two => CqlConsistency::Two,
            Self::Three => CqlConsistency::Three,
            Self::Quorum => CqlConsistency::Quorum,
            Self::All => CqlConsistency::All,
            Self::LocalQuorum => CqlConsistency::LocalQuorum,
            Self::LocalOne => CqlConsistency::LocalOne,
            Self::EachQuorum => CqlConsistency::EachQuorum,
            Self::Any => CqlConsistency::Any,
        }
    }

    /// Returns the default consistency for reads.
    pub fn default_read() -> Self {
        Self::LocalQuorum
    }

    /// Returns the default consistency for writes.
    pub fn default_write() -> Self {
        Self::LocalQuorum
    }
}

impl From<Consistency> for CqlConsistency {
    fn from(c: Consistency) -> Self {
        c.to_driver()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistency_default() {
        assert_eq!(Consistency::default(), Consistency::LocalQuorum);
    }

    #[test]
    fn test_consistency_to_driver() {
        assert!(matches!(Consistency::One.to_driver(), CqlConsistency::One));
        assert!(matches!(
            Consistency::LocalQuorum.to_driver(),
            CqlConsistency::LocalQuorum
        ));
    }
}
