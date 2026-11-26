/// Creates a tracing span for a YSQL query, with OpenTelemetry database semantic-convention fields.
pub fn create_query_span(operation: &str, table: &str, database: &str) -> tracing::Span {
    tracing::info_span!(
        "db.sql",
        db.system = "postgresql",
        db.operation = %operation,
        db.name = %database,
        "db.collection.name" = %table,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        otel.status_message = tracing::field::Empty,
    )
}
