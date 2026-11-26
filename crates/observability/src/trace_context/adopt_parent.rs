use tracing::debug;

use super::{Extractor, TraceParent, adopt_trace_parent};

/// Makes `span` a child of the trace context in `carrier`, when it holds a valid one.
///
/// For senders this process trusts. Call it before the span is entered: an adopt inside
/// `#[instrument]` runs too late and keeps the local parent. A rejection is logged by class;
/// without the `otlp` feature adoption is a no-op.
pub fn adopt_parent<C>(span: &tracing::Span, carrier: &C)
where
    C: Extractor + ?Sized,
{
    match TraceParent::from_carrier(carrier) {
        Ok(Some(parent)) => adopt_trace_parent(span, &parent),
        Ok(None) => {}
        Err(rejection) => debug!(
            rejection = rejection.class(),
            "an inbound trace context is not valid; the span keeps its local parent"
        ),
    }
}

#[cfg(all(test, feature = "otlp"))]
mod tests {
    use super::super::test_support::{Carrier, INBOUND, trace_id_of, trace_state_of, with_tracing};
    use super::*;

    #[test]
    fn an_adopted_parent_brings_no_trace_state() {
        let (trace_id, state) = with_tracing(|| {
            let carrier = Carrier(vec![
                ("traceparent".into(), INBOUND.into()),
                ("tracestate".into(), "x=jan@example.nl".into()),
            ]);
            let span = tracing::info_span!("callee");
            adopt_parent(&span, &carrier);
            (trace_id_of(&span), trace_state_of(&span))
        });

        assert_eq!(
            trace_id.as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert_eq!(state, "", "no trace state may be adopted from a sender");
    }

    #[test]
    fn a_malformed_context_leaves_the_local_parent_alone() {
        let (outer, inner) = with_tracing(|| {
            let outer = tracing::info_span!("outer");
            outer.in_scope(|| {
                let carrier = Carrier(vec![(
                    "traceparent".into(),
                    "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01".into(),
                )]);
                let inner = tracing::info_span!("inner");
                adopt_parent(&inner, &carrier);
                (trace_id_of(&outer), trace_id_of(&inner))
            })
        });

        assert_eq!(outer, inner);
    }
}
