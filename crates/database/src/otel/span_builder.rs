//! The [`DbSpanBuilder`] for database query spans.

use super::{DbOperation, DbSystem};

/// Builder for creating database spans with consistent attributes.
///
/// # Examples
///
/// ```rust
/// use fundamental_db::{DbSpanBuilder, DbSystem, DbOperation};
///
/// let span = DbSpanBuilder::new(DbSystem::SqlDb)
///     .operation(DbOperation::Select)
///     .database("myapp")
///     .table("users")
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct DbSpanBuilder {
    system: DbSystem,
    operation: Option<DbOperation>,
    database: Option<String>,
    table: Option<String>,
    statement: Option<String>,
}

impl DbSpanBuilder {
    /// Creates a new span builder for the given database system.
    pub fn new(system: DbSystem) -> Self {
        Self {
            system,
            operation: None,
            database: None,
            table: None,
            statement: None,
        }
    }

    /// Sets the database operation type.
    pub fn operation(mut self, op: DbOperation) -> Self {
        self.operation = Some(op);
        self
    }

    /// Sets the database name.
    pub fn database(mut self, name: impl Into<String>) -> Self {
        self.database = Some(name.into());
        self
    }

    /// Sets the table/collection name.
    pub fn table(mut self, name: impl Into<String>) -> Self {
        self.table = Some(name.into());
        self
    }

    /// Sets the query statement (optional, may contain sensitive data).
    pub fn statement(mut self, stmt: impl Into<String>) -> Self {
        self.statement = Some(stmt.into());
        self
    }

    /// Builds the tracing span with OTel attributes.
    pub fn build(self) -> tracing::Span {
        let op_str = self
            .operation
            .map(|o| o.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let db_name = self.database.as_deref().unwrap_or("unknown");
        let table_name = self.table.as_deref().unwrap_or("unknown");

        tracing::info_span!(
            "db.query",
            db.system = %self.system,
            db.operation = %op_str,
            db.name = %db_name,
            "db.collection.name" = %table_name,
            otel.kind = "client",
            otel.status_code = tracing::field::Empty,
            otel.status_message = tracing::field::Empty,
        )
    }
}
