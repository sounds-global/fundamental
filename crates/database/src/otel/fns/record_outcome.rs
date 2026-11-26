//! Records a database operation's outcome as OpenTelemetry status fields.

/// Records the outcome of a database operation on a span.
pub fn record_outcome(span: &tracing::Span, success: bool, error_message: Option<&str>) {
    if success {
        span.record("otel.status_code", "OK");
    } else {
        span.record("otel.status_code", "ERROR");
        if let Some(msg) = error_message {
            span.record("otel.status_message", msg);
        }
    }
}
