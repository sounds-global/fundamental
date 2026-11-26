use axum::body::Body;
use axum::http::{HeaderValue, Request};
use fundamental_server::{InboundRequestId, REQUEST_ID_HEADER, RequestIdResolution};
use tracing::warn;

/// Decides the id `request` is recorded under, and makes it the only `X-Request-Id` it carries.
pub(crate) fn resolve_request_id(
    mut request: Request<Body>,
    policy: InboundRequestId,
) -> Request<Body> {
    let headers = request.headers_mut();
    // Offered as a value the resolver refuses, so its provenance still says a caller sent one.
    let inbound = headers
        .get(REQUEST_ID_HEADER)
        .map(|value| value.to_str().unwrap_or_default());
    let resolution = RequestIdResolution::resolve(inbound, policy);
    match HeaderValue::from_str(&resolution.id().to_string()) {
        Ok(value) => {
            headers.insert(REQUEST_ID_HEADER, value);
        }
        // The caller's header must still not survive to be echoed or read.
        Err(error) => {
            headers.remove(REQUEST_ID_HEADER);
            warn!(%error, "a minted request id is not a valid header value; sending none");
        }
    }
    request.extensions_mut().insert(resolution);
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_server::{RequestIdProvenance, RequestIdRejection};

    const VALID: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    fn resolved(values: &[&[u8]], policy: InboundRequestId) -> (RequestIdResolution, Vec<String>) {
        let mut builder = Request::builder();
        for value in values {
            builder = builder.header(
                REQUEST_ID_HEADER,
                HeaderValue::from_bytes(value).expect("a header value"),
            );
        }
        let request = resolve_request_id(builder.body(Body::empty()).expect("a request"), policy);
        let resolution = *request
            .extensions()
            .get::<RequestIdResolution>()
            .expect("the decision rides in the extensions");
        let headers = request
            .headers()
            .get_all(REQUEST_ID_HEADER)
            .iter()
            .map(|value| value.to_str().expect("ascii").to_owned())
            .collect();
        (resolution, headers)
    }

    #[test]
    fn a_request_with_no_id_gets_a_minted_one_in_its_header() {
        let (resolution, headers) = resolved(&[], InboundRequestId::Ignore);
        assert_eq!(resolution.provenance(), RequestIdProvenance::Minted);
        assert_eq!(headers, [resolution.id().to_string()]);
    }

    #[test]
    fn a_caller_s_id_is_replaced_in_the_header_by_default() {
        let (resolution, headers) = resolved(&[VALID.as_bytes()], InboundRequestId::Ignore);
        assert_eq!(resolution.provenance(), RequestIdProvenance::Ignored);
        assert_eq!(headers, [resolution.id().to_string()]);
        assert_ne!(headers, [VALID]);
    }

    #[test]
    fn a_trusted_caller_s_id_is_kept() {
        let (resolution, headers) = resolved(&[VALID.as_bytes()], InboundRequestId::Adopt);
        assert_eq!(resolution.provenance(), RequestIdProvenance::Adopted);
        assert_eq!(headers, [VALID]);
    }

    #[test]
    fn a_second_value_never_survives() {
        let (resolution, headers) = resolved(
            &[VALID.as_bytes(), b"jan@example.nl"],
            InboundRequestId::Adopt,
        );
        assert_eq!(headers, [resolution.id().to_string()]);
    }

    #[test]
    fn a_value_that_is_not_ascii_is_rejected_by_class() {
        let (resolution, headers) = resolved(&[b"\xffjan"], InboundRequestId::Adopt);
        assert_eq!(
            resolution.provenance(),
            RequestIdProvenance::Rejected(RequestIdRejection::Malformed)
        );
        assert_eq!(headers, [resolution.id().to_string()]);
    }
}
