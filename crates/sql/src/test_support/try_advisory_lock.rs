use sqlx::postgres::PgConnection;

/// Takes the session advisory lock `key` on `connection` if free, in the two-`int4` space held names use.
///
/// # Errors
/// The underlying `sqlx` error if the statement fails.
pub(super) async fn try_advisory_lock(
    connection: &mut PgConnection,
    (class, object): (i32, i32),
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_try_advisory_lock($1, $2)")
        .bind(class)
        .bind(object)
        .fetch_one(connection)
        .await
}
