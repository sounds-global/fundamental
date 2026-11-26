//! Query value binding for YSQL.

/// Queries that can have values bound to them.
pub trait BindValues<'q>: Sized {
    /// Binds a value to the query.
    fn bind_value<T: sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + 'q>(
        self,
        value: T,
    ) -> Self;
}

impl<'q> BindValues<'q> for sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    fn bind_value<T: sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + 'q>(
        self,
        value: T,
    ) -> Self {
        self.bind(value)
    }
}
