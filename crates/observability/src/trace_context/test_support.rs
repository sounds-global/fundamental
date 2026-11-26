//! Carriers and subscribers the trace-context tests share.

use super::{Extractor, Injector};

/// A well-formed W3C `traceparent` naming a trace no test process started.
#[cfg(feature = "otlp")]
pub(super) const INBOUND: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[derive(Default)]
pub(super) struct Carrier(pub(super) Vec<(String, String)>);

impl Injector for Carrier {
    fn set(&mut self, key: &str, value: String) {
        self.0.retain(|(name, _)| name != key);
        self.0.push((key.to_string(), value));
    }
}

impl Extractor for Carrier {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_str()).collect()
    }
}

/// Runs `body` under a scoped subscriber with a real OpenTelemetry layer.
#[cfg(feature = "otlp")]
pub(super) fn with_tracing<T>(body: impl FnOnce() -> T) -> T {
    with_tracing_sampled(opentelemetry_sdk::trace::Sampler::AlwaysOn, body)
}

/// [`with_tracing`], under `sampler`.
#[cfg(feature = "otlp")]
pub(super) fn with_tracing_sampled<T>(
    sampler: opentelemetry_sdk::trace::Sampler,
    body: impl FnOnce() -> T,
) -> T {
    use opentelemetry::trace::TracerProvider as _;
    use tracing_subscriber::layer::SubscriberExt;

    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_sampler(sampler)
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("test")));
    tracing::subscriber::with_default(subscriber, body)
}

/// The trace id a span belongs to, as the exporter would see it.
#[cfg(feature = "otlp")]
pub(super) fn trace_id_of(span: &tracing::Span) -> Option<String> {
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let context = span.context();
    let span_context = context.span().span_context().clone();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

/// The trace state a span carries, in its `tracestate` header form.
#[cfg(feature = "otlp")]
pub(super) fn trace_state_of(span: &tracing::Span) -> String {
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    span.context().span().span_context().trace_state().header()
}
