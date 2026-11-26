use super::TraceParent;

/// Makes `span` a child of the span `parent` names, continuing the sender's trace.
///
/// For a sender this process trusts; a caller is [linked](super::link_trace_parent) instead.
/// Its `tracestate` is never taken. Call it before the span is entered. A no-op without the
/// `otlp` feature.
#[cfg(feature = "otlp")]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn adopt_trace_parent(span: &tracing::Span, parent: &TraceParent) {
    use opentelemetry::Context;
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::{OpenTelemetrySpanExt, SetParentError};

    let remote = Context::new()
        .with_remote_span_context(super::remote_span_context(parent, parent.is_sampled()));
    match span.set_parent(remote) {
        Ok(()) => {}
        // The trace is severed here, and nothing else would report it.
        Err(SetParentError::AlreadyStarted) => tracing::warn!(
            "an incoming trace context arrived after its span was entered; \
             adopt the parent before entering the span, or the trace is severed here"
        ),
        // Routine: export is off or the span is filtered out.
        Err(reason) => tracing::trace!(%reason, "incoming trace context not adopted"),
    }
}

/// Adopts `parent` as `span`'s parent; a no-op in this build.
#[cfg(not(feature = "otlp"))]
pub fn adopt_trace_parent(_span: &tracing::Span, _parent: &TraceParent) {}

#[cfg(all(test, feature = "otlp"))]
mod tests {
    use super::super::link_trace_parent;
    use super::super::test_support::{INBOUND, with_tracing_sampled};
    use super::*;

    /// Whether a span given a sampled inbound context by `policy` is sampled, under a
    /// parent-based sampler whose own root decision is "never".
    fn sampled_under(policy: fn(&tracing::Span, &TraceParent)) -> bool {
        use opentelemetry::trace::TraceContextExt;
        use opentelemetry_sdk::trace::Sampler;
        use tracing_opentelemetry::OpenTelemetrySpanExt;

        with_tracing_sampled(Sampler::ParentBased(Box::new(Sampler::AlwaysOff)), || {
            let span = tracing::info_span!("request");
            policy(&span, &TraceParent::parse(INBOUND).expect("valid"));
            let _entered = span.enter();
            span.context().span().span_context().is_sampled()
        })
    }

    #[test]
    fn only_an_adopted_parent_s_flag_decides_sampling() {
        assert!(
            sampled_under(adopt_trace_parent),
            "a trusted parent's flag is honoured"
        );
        assert!(
            !sampled_under(link_trace_parent),
            "a linked caller cannot force its request into the trace store"
        );
    }
}
