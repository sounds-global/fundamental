/// Creates a tracing span for a lightweight transaction (LWT).
pub fn create_lwt_span(operation: &str, table: &str, keyspace: &str) -> tracing::Span {
    tracing::info_span!(
        "db.cql.lwt",
        db.system = "cassandra",
        db.operation = %operation,
        db.name = %keyspace,
        "db.collection.name" = %table,
        "db.cql.lwt" = true,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        "db.cql.lwt_applied" = tracing::field::Empty,
    )
}
