/// Creates a tracing span for a YCQL batch operation.
pub fn create_batch_span(
    batch_type: &str,
    table: &str,
    keyspace: &str,
    item_count: usize,
) -> tracing::Span {
    tracing::info_span!(
        "db.cql.batch",
        db.system = "cassandra",
        db.operation = "batch",
        db.name = %keyspace,
        "db.collection.name" = %table,
        "db.cql.batch_type" = %batch_type,
        "db.cql.batch_size" = item_count,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
    )
}
