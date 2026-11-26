//! [`Consistency`]: the freshness a check requests.

use serde::{Deserialize, Serialize};

use super::{HybridTs, Zookie};

/// How fresh the tuples a check reads must be.
///
/// What the engine can guarantee depends on the backend. Serializable for the standalone
/// service's RPC.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Consistency {
    /// Prefer speed, accepting bounded staleness. The default.
    #[default]
    MinimizeLatency,
    /// Read state at least as new as this zookie, the defense against the "new enemy" problem.
    AtLeastAsFresh(Zookie),
    /// Read the freshest consistent snapshot ("now"), the strongest guarantee.
    FullConsistency,
}

#[cfg(feature = "config")]
impl From<fundamental_config::settings::AuthzConsistency> for Consistency {
    /// `AtLeastAsFresh` carries a token, so it is never a configurable default.
    fn from(mode: fundamental_config::settings::AuthzConsistency) -> Self {
        use fundamental_config::settings::AuthzConsistency;
        match mode {
            AuthzConsistency::MinimizeLatency => Consistency::MinimizeLatency,
            AuthzConsistency::FullConsistency => Consistency::FullConsistency,
        }
    }
}

impl Consistency {
    /// The minimum timestamp reads must reflect, if the request pins one.
    #[must_use]
    pub fn floor(&self) -> Option<HybridTs> {
        match self {
            Consistency::AtLeastAsFresh(zookie) => Some(zookie.ts()),
            _ => None,
        }
    }

    /// Whether this request demands a snapshot-pinned read.
    #[must_use]
    pub fn is_strong(&self) -> bool {
        matches!(
            self,
            Consistency::FullConsistency | Consistency::AtLeastAsFresh(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaHash;
    use crate::zookie::BackendTag;

    #[test]
    fn floor_and_strength() {
        assert!(Consistency::MinimizeLatency.floor().is_none());
        assert!(!Consistency::MinimizeLatency.is_strong());
        assert!(Consistency::FullConsistency.is_strong());

        let zookie = Zookie::new(BackendTag::SqlDb, HybridTs::new(9, 0), SchemaHash::empty());
        let at_least = Consistency::AtLeastAsFresh(zookie);
        assert_eq!(at_least.floor(), Some(HybridTs::new(9, 0)));
        assert!(at_least.is_strong());
    }
}
