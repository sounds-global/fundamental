use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState};

use super::TraceParent;

/// The remote span context `parent` names, with an empty trace state and `sampled` its only flag.
pub(super) fn remote_span_context(parent: &TraceParent, sampled: bool) -> SpanContext {
    let flags = if sampled {
        TraceFlags::SAMPLED
    } else {
        TraceFlags::default()
    };
    SpanContext::new(
        TraceId::from_bytes(parent.trace_id()),
        SpanId::from_bytes(parent.parent_id()),
        flags,
        true,
        TraceState::default(),
    )
}
