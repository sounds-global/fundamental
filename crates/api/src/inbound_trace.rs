use axum::body::Body;
use axum::http::Request;
use fundamental_observability::trace_context::{TraceParent, TraceParentRejection};
use fundamental_server::{InboundTraceContext, RequestHeaders};

/// What a request's `traceparent` parsed as, and what the deployment lets it do.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InboundTrace {
    policy: InboundTraceContext,
    parsed: Result<Option<TraceParent>, TraceParentRejection>,
}

impl InboundTrace {
    /// Reads `request`'s `traceparent` under `policy` and attaches the result to its extensions.
    pub(crate) fn attach(mut request: Request<Body>, policy: InboundTraceContext) -> Request<Body> {
        let headers: RequestHeaders = request
            .headers()
            .get_all("traceparent")
            .iter()
            .take(1)
            // Offered as a value the parser refuses, so the rejection is still logged by class.
            .map(|value| {
                (
                    "traceparent".to_owned(),
                    value.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let parsed = TraceParent::from_carrier(&headers);
        request.extensions_mut().insert(Self { policy, parsed });
        request
    }

    /// Meets the context on `span` — see [`InboundTraceContext::meet`].
    pub(crate) fn meet(self, span: &tracing::Span) {
        self.policy.meet(span, self.parsed);
    }
}
