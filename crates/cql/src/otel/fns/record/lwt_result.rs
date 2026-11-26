/// Records the result of a lightweight transaction.
pub fn record_lwt_result(span: &tracing::Span, applied: bool) {
    span.record("db.cql.lwt_applied", applied);
    span.record("otel.status_code", "OK");
}
