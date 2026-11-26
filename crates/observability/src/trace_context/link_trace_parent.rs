use super::TraceParent;

/// Links `span` to the span `parent` names, without joining its trace.
///
/// For an untrusted sender: the span keeps its own parent and carries the sender's two ids as
/// an OpenTelemetry link, so the sender neither grafts it into its trace nor decides its
/// sampling. Call it before the span is entered. A no-op without the `otlp` feature.
#[cfg(feature = "otlp")]
pub fn link_trace_parent(span: &tracing::Span, parent: &TraceParent) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    span.add_link(super::remote_span_context(parent, false));
}

/// Links `span` to `parent`; a no-op in this build.
#[cfg(not(feature = "otlp"))]
pub fn link_trace_parent(_span: &tracing::Span, _parent: &TraceParent) {}

#[cfg(all(test, feature = "otlp"))]
mod tests {
    use super::super::test_support::{INBOUND, trace_id_of, with_tracing};
    use super::*;

    #[test]
    fn a_linked_context_does_not_become_the_parent() {
        let (outer, inner) = with_tracing(|| {
            let outer = tracing::info_span!("outer");
            outer.in_scope(|| {
                let inner = tracing::info_span!("inner");
                let parent = TraceParent::parse(INBOUND).expect("valid");
                link_trace_parent(&inner, &parent);
                (trace_id_of(&outer), trace_id_of(&inner))
            })
        });

        assert_eq!(
            outer, inner,
            "a link must not move the span into the sender's trace"
        );
        assert_ne!(inner.as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
    }
}
