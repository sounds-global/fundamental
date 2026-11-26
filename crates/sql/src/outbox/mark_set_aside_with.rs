use chrono::{DateTime, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::SqlModel;
use fundamental_core::outbox::{OutboxColumns, RefusalKind};
use fundamental_db::SchemaName;
use fundamental_encryption::KeyManager;
use sqlx::Postgres;

use super::{ClaimedKey, SetAsideRow};
use crate::query::pg_query;
use crate::{SqlError, SqlResult};

/// Sets the row `key` addresses aside, recording `kind`, `at` and their seal in the framework's own columns.
///
/// # Errors
/// [`SqlError::Config`] without a key manager to seal the decision, [`SqlError::Other`] when the
/// seal cannot be made, [`SqlError::OutboxMarkMissed`] when the write does not land on exactly one
/// row, or the driver's error.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) async fn mark_set_aside_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    key: &ClaimedKey,
    kind: RefusalKind,
    at: DateTime<Utc>,
) -> SqlResult<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel,
{
    let manager = enc.ok_or_else(|| {
        SqlError::Config(format!(
            "setting a {} row aside needs a key manager to seal the decision",
            M::TABLE
        ))
    })?;
    // The column keeps microseconds, so the seal covers the instant as it reads back.
    let at_micros = at.timestamp_micros();
    let at = DateTime::from_timestamp_micros(at_micros).ok_or_else(|| {
        SqlError::Query(format!(
            "a set-aside instant for {} is out of range",
            M::TABLE
        ))
    })?;
    let parts = SetAsideRow::seal_parts(key, at_micros, kind);
    let seal = manager
        .seal_outbox_set_aside(M::TABLE, &parts)
        .map_err(|error| SqlError::Other(AnyError::new(error)))?;

    let (predicate, values) = key.predicate(4);
    let sql = format!(
        "UPDATE {} SET {} = $1, {} = $2, {} = $3 WHERE {predicate} AND {} IS NULL",
        schema.qualify(M::TABLE),
        OutboxColumns::SET_ASIDE_AT,
        OutboxColumns::SET_ASIDE_REASON,
        OutboxColumns::SET_ASIDE_SEAL,
        OutboxColumns::SET_ASIDE_AT,
    );
    let mut write = pg_query(sql).bind(at).bind(kind.code()).bind(seal.to_vec());
    for value in values {
        write = write.bind(value);
    }
    let affected = write
        .execute(exec)
        .await
        .map_err(SqlError::from)?
        .rows_affected();
    if affected == 1 {
        Ok(())
    } else {
        Err(SqlError::OutboxMarkMissed {
            table: M::TABLE,
            affected,
        })
    }
}
