/// Creates a tracing span for a YCQL operation.
pub fn create_db_span(operation: &str, table: &str, keyspace: &str) -> tracing::Span {
    tracing::info_span!(
        "db.cql",
        db.system = "cassandra",
        db.operation = %operation,
        db.name = %keyspace,
        "db.collection.name" = %table,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        otel.status_message = tracing::field::Empty,
    )
}
