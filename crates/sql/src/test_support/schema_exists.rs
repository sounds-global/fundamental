use super::is_inlinable;
use crate::{SqlPool, SqlResult};

/// Whether `schema` is in the catalog; `SET search_path` to a missing schema does not error.
///
/// A name that is not plain identifier text answers `false` without contacting the database.
///
/// # Errors
/// Returns the underlying [`SqlError`](crate::SqlError) if the catalog cannot be read.
pub async fn schema_exists(pool: &SqlPool, schema: &str) -> SqlResult<bool> {
    if !is_inlinable(schema) {
        eprintln!("schema: refusing to inline '{schema}'; it is reported as absent");
        return Ok(false);
    }
    let found = pool
        .fetch_optional(&format!(
            "SELECT 1 FROM information_schema.schemata WHERE schema_name = '{schema}'"
        ))
        .await?;
    Ok(found.is_some())
}
