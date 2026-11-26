use super::Injector;

/// Writes the current span's trace context into `carrier`, for every outbound hop.
///
/// Without the `otlp` feature it writes nothing: a `traceparent` naming an unrecorded span is
/// worse than none.
#[cfg(feature = "otlp")]
pub fn inject_current<C>(carrier: &mut C)
where
    C: Injector + ?Sized,
{
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let context = tracing::Span::current().context();
    let mut adapter = super::adapter::InjectorAdapter(carrier);
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut adapter);
    });
}

/// Writes the current span's trace context into `carrier`; a no-op in this build.
#[cfg(not(feature = "otlp"))]
pub fn inject_current<C>(_carrier: &mut C)
where
    C: Injector + ?Sized,
{
}
