/// Creates a tracing span for a batch operation of `item_count` items.
pub fn create_batch_span(
    operation: &str,
    table: &str,
    database: &str,
    item_count: usize,
) -> tracing::Span {
    tracing::info_span!(
        "db.sql.batch",
        db.system = "postgresql",
        db.operation = "batch",
        "db.sql.batch_operation" = %operation,
        db.name = %database,
        "db.collection.name" = %table,
        "db.sql.batch_size" = item_count,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
    )
}
