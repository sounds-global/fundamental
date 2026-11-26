use crate::RequestIdRejection;

/// Where the id a request is recorded under came from, logged by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestIdProvenance {
    /// This server minted it; the caller sent none.
    Minted,
    /// The caller's id, recorded because the deployment trusts its callers.
    Adopted,
    /// The caller sent an id; this server minted its own, by policy.
    Ignored,
    /// The caller sent something that is not an id; this server minted its own.
    Rejected(RequestIdRejection),
}

impl RequestIdProvenance {
    /// The provenance as a fixed, lower-case word — the value a log field records.
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Minted => "minted",
            Self::Adopted => "adopted",
            Self::Ignored => "ignored",
            Self::Rejected(rejection) => rejection.class(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejection_is_recorded_as_its_own_class() {
        assert_eq!(RequestIdProvenance::Minted.class(), "minted");
        assert_eq!(RequestIdProvenance::Adopted.class(), "adopted");
        assert_eq!(RequestIdProvenance::Ignored.class(), "ignored");
        assert_eq!(
            RequestIdProvenance::Rejected(RequestIdRejection::Oversized).class(),
            "oversized"
        );
    }
}
