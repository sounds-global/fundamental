use fundamental_observability::{RequestId, RequestIdRejection};
use tracing::debug;
use tracing::field::{DisplayValue, display};

use crate::message::MessageHeaders;
use crate::tracing::{HEADER_REQUEST_ID, HEADER_TENANT_ID, HEADER_TRACE_ID, HEADER_USER_ID};

/// What a received message's correlation headers may put on its delivery span.
///
/// Only the request id, when a valid [`RequestId`]; the rest are left off and logged by class,
/// never by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeliveryIds {
    request_id: Result<Option<RequestId>, RequestIdRejection>,
    unrecorded: [Option<&'static str>; 3],
}

impl DeliveryIds {
    /// Reads the correlation headers of a message.
    pub(crate) fn from_headers(headers: &MessageHeaders) -> Self {
        let present = |name: &'static str| headers.get(name).map(|_| name);
        Self {
            request_id: headers
                .get(HEADER_REQUEST_ID)
                .map(RequestId::parse)
                .transpose(),
            unrecorded: [
                present(HEADER_USER_ID),
                present(HEADER_TENANT_ID),
                present(HEADER_TRACE_ID),
            ],
        }
    }

    /// The request id, ready to record, when the publisher sent a valid one.
    pub(crate) fn request_id(&self) -> Option<DisplayValue<RequestId>> {
        self.request_id.ok().flatten().map(display)
    }

    /// Logs, inside the delivery span, each correlation header that was left off
    /// it and why — by header name and class, never by value.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) fn log(&self) {
        if let Err(rejection) = self.request_id {
            debug!(
                header = HEADER_REQUEST_ID,
                rejected = rejection.class(),
                "a message's request id is not a UUID; the span records none"
            );
        }
        for header in self.unrecorded.into_iter().flatten() {
            debug!(
                header,
                "a message's correlation header is not recorded on the delivery span"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    fn headers(pairs: &[(&str, &str)]) -> MessageHeaders {
        let mut headers = MessageHeaders::new();
        for (name, value) in pairs {
            headers.insert(*name, (*value).to_owned());
        }
        headers
    }

    fn recorded(ids: &DeliveryIds) -> Option<String> {
        ids.request_id().map(|id| id.to_string())
    }

    #[test]
    fn a_uuid_request_id_is_recorded() {
        let ids = DeliveryIds::from_headers(&headers(&[(HEADER_REQUEST_ID, REQUEST)]));
        assert_eq!(recorded(&ids).as_deref(), Some(REQUEST));
    }

    #[test]
    fn anything_but_a_uuid_is_left_off_by_class() {
        for (value, rejection) in [
            ("req-789", RequestIdRejection::Malformed),
            ("jan@example.nl", RequestIdRejection::Malformed),
            (
                "00000000-0000-0000-0000-000000000000",
                RequestIdRejection::Reserved,
            ),
        ] {
            let ids = DeliveryIds::from_headers(&headers(&[(HEADER_REQUEST_ID, value)]));
            assert_eq!(recorded(&ids), None, "{value}");
            assert_eq!(ids.request_id, Err(rejection), "{value}");
        }
    }

    #[test]
    fn user_tenant_and_trace_ids_are_never_recorded() {
        let ids = DeliveryIds::from_headers(&headers(&[
            (HEADER_USER_ID, "0b8f7c6e-6a0e-4d1b-9a53-2f3c8f0f3a11"),
            (HEADER_TENANT_ID, "jan@example.nl"),
            (HEADER_TRACE_ID, "anything"),
        ]));
        assert_eq!(recorded(&ids), None);
        assert_eq!(
            ids.unrecorded,
            [
                Some(HEADER_USER_ID),
                Some(HEADER_TENANT_ID),
                Some(HEADER_TRACE_ID)
            ]
        );
    }
}
