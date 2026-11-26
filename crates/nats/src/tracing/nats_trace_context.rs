use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::message::MessageHeaders;

/// Header carrying the trace id.
pub const HEADER_TRACE_ID: &str = "Fundamental-Trace-Id";
/// Header carrying the span id.
pub const HEADER_SPAN_ID: &str = "Fundamental-Span-Id";
/// Header carrying the parent span id.
pub const HEADER_PARENT_SPAN_ID: &str = "Fundamental-Parent-Span-Id";
/// Header carrying the request id.
pub const HEADER_REQUEST_ID: &str = "Fundamental-Request-Id";
/// Header carrying the user id.
pub const HEADER_USER_ID: &str = "Fundamental-User-Id";
/// Header carrying the session id.
pub const HEADER_SESSION_ID: &str = "Fundamental-Session-Id";
/// Header carrying the tenant id.
pub const HEADER_TENANT_ID: &str = "Fundamental-Tenant-Id";
/// W3C `traceparent`, the header that carries the real distributed trace.
///
/// Owned by [`trace_context`](fundamental_observability::trace_context), never by
/// [`inject_trace_context`], so no fabricated parent is written.
pub const HEADER_TRACEPARENT: &str = "traceparent";

/// Distributed-trace context propagated through NATS message headers, so a span
/// started in one service continues in the service that consumes the message.
#[derive(Debug, Clone, Default)]
pub struct NatsTraceContext {
    /// The trace id correlating the whole request.
    pub trace_id: Option<String>,
    /// The current span id.
    pub span_id: Option<String>,
    /// The parent span id.
    pub parent_span_id: Option<String>,
    /// The logical request id.
    pub request_id: Option<String>,
    /// The authenticated user id.
    pub user_id: Option<String>,
    /// The session id.
    pub session_id: Option<String>,
    /// The tenant id.
    pub tenant_id: Option<String>,
}

impl NatsTraceContext {
    /// Creates an empty context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the trace id.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Sets the span id.
    #[must_use]
    pub fn with_span_id(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }

    /// Sets the request id.
    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Sets the user id.
    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Returns whether both a trace id and span id are present.
    #[must_use]
    pub fn has_trace(&self) -> bool {
        self.trace_id.is_some() && self.span_id.is_some()
    }
}

/// Writes the correlation fields of `context` into `headers`.
///
/// The correlation baggage half of a hop; `traceparent` is left to
/// [`trace_context::inject_current`](fundamental_observability::trace_context::inject_current).
pub fn inject_trace_context(context: &NatsTraceContext, headers: &mut MessageHeaders) {
    let fields = [
        (HEADER_TRACE_ID, &context.trace_id),
        (HEADER_SPAN_ID, &context.span_id),
        (HEADER_PARENT_SPAN_ID, &context.parent_span_id),
        (HEADER_REQUEST_ID, &context.request_id),
        (HEADER_USER_ID, &context.user_id),
        (HEADER_SESSION_ID, &context.session_id),
        (HEADER_TENANT_ID, &context.tenant_id),
    ];
    for (name, value) in fields {
        if let Some(value) = value {
            headers.insert(name, value.clone());
        }
    }
}

/// Reads a [`NatsTraceContext`] back out of `headers`.
#[must_use]
pub fn extract_trace_context(headers: &MessageHeaders) -> NatsTraceContext {
    NatsTraceContext {
        trace_id: headers.get(HEADER_TRACE_ID).map(String::from),
        span_id: headers.get(HEADER_SPAN_ID).map(String::from),
        parent_span_id: headers.get(HEADER_PARENT_SPAN_ID).map(String::from),
        request_id: headers.get(HEADER_REQUEST_ID).map(String::from),
        user_id: headers.get(HEADER_USER_ID).map(String::from),
        session_id: headers.get(HEADER_SESSION_ID).map(String::from),
        tenant_id: headers.get(HEADER_TENANT_ID).map(String::from),
    }
}

/// Derives a child context: a fresh span id, the parent's span id as the parent,
/// and the rest of the baggage carried forward.
#[must_use]
pub fn create_child_context(parent: &NatsTraceContext) -> NatsTraceContext {
    NatsTraceContext {
        trace_id: parent.trace_id.clone(),
        span_id: Some(generate_span_id()),
        parent_span_id: parent.span_id.clone(),
        request_id: parent.request_id.clone(),
        user_id: parent.user_id.clone(),
        session_id: parent.session_id.clone(),
        tenant_id: parent.tenant_id.clone(),
    }
}

fn generate_span_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}", timestamp ^ (counter << 48))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_then_extract_roundtrips() {
        let context = NatsTraceContext::new()
            .with_trace_id("trace-123")
            .with_span_id("span-456")
            .with_request_id("req-789")
            .with_user_id("user-abc");
        let mut headers = MessageHeaders::new();
        inject_trace_context(&context, &mut headers);

        let extracted = extract_trace_context(&headers);
        assert_eq!(extracted.trace_id.as_deref(), Some("trace-123"));
        assert_eq!(extracted.span_id.as_deref(), Some("span-456"));
        assert_eq!(extracted.request_id.as_deref(), Some("req-789"));
        assert_eq!(extracted.user_id.as_deref(), Some("user-abc"));
    }

    #[test]
    fn child_keeps_trace_and_links_parent() {
        let parent = NatsTraceContext::new()
            .with_trace_id("trace-123")
            .with_span_id("span-456")
            .with_request_id("req-789");
        let child = create_child_context(&parent);
        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.parent_span_id, parent.span_id);
        assert_eq!(child.request_id, parent.request_id);
    }

    #[test]
    fn has_trace_needs_both_ids() {
        assert!(!NatsTraceContext::new().has_trace());
        assert!(!NatsTraceContext::new().with_trace_id("t").has_trace());
        assert!(
            NatsTraceContext::new()
                .with_trace_id("t")
                .with_span_id("s")
                .has_trace()
        );
    }

    /// The baggage half never writes `traceparent`.
    #[test]
    fn the_baggage_half_never_writes_the_w3c_header() {
        let context = NatsTraceContext::new()
            .with_trace_id("0af7651916cd43dd8448eb211c80319c")
            .with_span_id("b7ad6b7169203331");
        let mut headers = MessageHeaders::new();
        inject_trace_context(&context, &mut headers);

        assert_eq!(headers.get(HEADER_TRACEPARENT), None);
        assert_eq!(
            headers.get(HEADER_TRACE_ID),
            Some("0af7651916cd43dd8448eb211c80319c"),
            "the correlation fields it does own are still carried"
        );
    }
}
