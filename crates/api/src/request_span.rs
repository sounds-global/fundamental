use axum::http::{Method, Request};
use fundamental_server::RequestIdResolution;

use crate::inbound_trace::InboundTrace;
use tracing::{Span, field, info_span};

/// The outermost span of one request, recording only values this server produced.
///
/// Never the URI or a header. The caller's trace context is met here, so every span inside
/// inherits it.
pub(crate) fn request_span<B>(request: &Request<B>) -> Span {
    let resolution = request.extensions().get::<RequestIdResolution>();
    let span = info_span!(
        "http.request",
        method = method_class(request.method()),
        request_id = field::Empty,
        request_id_provenance = field::Empty,
    );
    if let Some(resolution) = resolution {
        span.record("request_id", field::display(resolution.id()));
        span.record("request_id_provenance", resolution.provenance().class());
    }
    if let Some(inbound) = request.extensions().get::<InboundTrace>() {
        inbound.meet(&span);
    }
    span
}

/// The method as a fixed token: one of the nine RFC 9110 defines, or `extension`.
fn method_class(method: &Method) -> &'static str {
    match method.as_str() {
        "GET" => "GET",
        "HEAD" => "HEAD",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "CONNECT" => "CONNECT",
        "OPTIONS" => "OPTIONS",
        "TRACE" => "TRACE",
        "PATCH" => "PATCH",
        _ => "extension",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_method_is_recorded_as_its_token() {
        assert_eq!(method_class(&Method::GET), "GET");
        assert_eq!(method_class(&Method::PATCH), "PATCH");
        assert_eq!(method_class(&Method::TRACE), "TRACE");
    }

    #[test]
    fn an_extension_method_is_recorded_as_a_class() {
        for token in ["JAN%40EXAMPLE.NL", "9F86D081884C7D659A2FEAA0C55AD015"] {
            let method = Method::from_bytes(token.as_bytes()).expect("a valid token");
            assert_eq!(method_class(&method), "extension");
        }
    }
}
