/// Creates a tracing span for a YSQL transaction operation: begin, commit or rollback.
pub fn create_transaction_span(operation: &str, database: &str) -> tracing::Span {
    tracing::info_span!(
        "db.sql.transaction",
        db.system = "postgresql",
        db.operation = %operation,
        db.name = %database,
        otel.kind = "client",
        otel.status_code = tracing::field::Empty,
        otel.status_message = tracing::field::Empty,
    )
}
