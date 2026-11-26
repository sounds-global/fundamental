use tracing::{debug, info};

use crate::{InboundRequestId, RequestId, RequestIdProvenance};

/// The name of the header a request id travels in, in both directions.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// The id one request is recorded under, decided once at the outermost edge, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestIdResolution {
    id: RequestId,
    provenance: RequestIdProvenance,
}

impl RequestIdResolution {
    /// Decides the id for a request that carried `inbound` in its `X-Request-Id`.
    #[must_use]
    pub fn resolve(inbound: Option<&str>, policy: InboundRequestId) -> Self {
        let Some(inbound) = inbound else {
            return Self::minted();
        };
        let provenance = match policy {
            InboundRequestId::Ignore => RequestIdProvenance::Ignored,
            InboundRequestId::Adopt => match RequestId::parse(inbound) {
                Ok(id) => {
                    return Self {
                        id,
                        provenance: RequestIdProvenance::Adopted,
                    };
                }
                Err(rejection) => RequestIdProvenance::Rejected(rejection),
            },
        };
        Self {
            id: RequestId::mint(),
            provenance,
        }
    }

    /// A freshly minted id, for a request that arrived with none.
    #[must_use]
    pub fn minted() -> Self {
        Self {
            id: RequestId::mint(),
            provenance: RequestIdProvenance::Minted,
        }
    }

    /// The id the request is recorded under.
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    /// Where the id came from.
    #[must_use]
    pub const fn provenance(&self) -> RequestIdProvenance {
        self.provenance
    }

    /// Logs how the id was decided, by class, into the current span.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn log(&self) {
        match self.provenance {
            RequestIdProvenance::Rejected(rejection) => info!(
                rejected = rejection.class(),
                "replaced an inbound request id that is not a UUID with this server's own"
            ),
            RequestIdProvenance::Ignored => {
                debug!("an inbound request id was ignored; this deployment records its own")
            }
            RequestIdProvenance::Minted | RequestIdProvenance::Adopted => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequestIdRejection;

    const VALID: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    #[test]
    fn no_inbound_id_mints_one_under_either_policy() {
        for policy in [InboundRequestId::Ignore, InboundRequestId::Adopt] {
            let resolved = RequestIdResolution::resolve(None, policy);
            assert_eq!(resolved.provenance(), RequestIdProvenance::Minted);
        }
    }

    /// The default refuses even a well-formed UUID: it may be a session id.
    #[test]
    fn a_valid_inbound_id_is_ignored_by_default() {
        let resolved = RequestIdResolution::resolve(Some(VALID), InboundRequestId::default());
        assert_eq!(resolved.provenance(), RequestIdProvenance::Ignored);
        assert_ne!(resolved.id().to_string(), VALID);
    }

    #[test]
    fn a_valid_inbound_id_is_adopted_when_the_callers_are_trusted() {
        let resolved = RequestIdResolution::resolve(Some(VALID), InboundRequestId::Adopt);
        assert_eq!(resolved.provenance(), RequestIdProvenance::Adopted);
        assert_eq!(resolved.id().to_string(), VALID);
    }

    #[test]
    fn a_hostile_inbound_id_is_replaced_even_when_the_callers_are_trusted() {
        let cases = [
            ("jan@example.nl", RequestIdRejection::Malformed),
            (
                "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                RequestIdRejection::Oversized,
            ),
            ("\u{7}\u{1b}[2J", RequestIdRejection::Malformed),
            (
                "00000000-0000-0000-0000-000000000000",
                RequestIdRejection::Reserved,
            ),
        ];
        for (value, rejection) in cases {
            let resolved = RequestIdResolution::resolve(Some(value), InboundRequestId::Adopt);
            assert_eq!(
                resolved.provenance(),
                RequestIdProvenance::Rejected(rejection),
                "{value:?}"
            );
            assert_ne!(resolved.id().to_string(), value);
        }
    }
}
