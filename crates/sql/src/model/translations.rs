//! Companion-table persistence for translatable (`Localized<…>`) model fields.
//!
//! A row and its companion rows are written, deleted and read by one statement, and every table
//! name is schema-qualified, so a like-named table in an earlier schema cannot answer.

use fundamental_core::i18n::{Checked, Locale, TranslationCheck, TranslationRow};
use fundamental_core::model::SqlModel;
use fundamental_db::{DatabaseError, DatabaseResult, SchemaName};
use fundamental_encryption::KeyManager;
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{Decode, Postgres, Row, Type};
use tracing::instrument;

use crate::connection::SqlConn;
use crate::query::pg_query;
use crate::{FromRow, SqlError, ToRow};

/// The column a read selects beside the row's own: its companion rows as one JSON array, or `NULL`.
const TRANSLATIONS: &str = "fundamental_translations";

fn companion<M: SqlModel>(schema: &SchemaName) -> Option<String> {
    M::translation_table().map(|table| schema.qualify(table))
}

fn key_join<M: SqlModel>(left: &str, right: &str) -> String {
    M::KEY_COLUMNS
        .iter()
        .map(|column| format!("{left}.{column} = {right}.{column}"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `write`, ending in `RETURNING *`, extended to write `M`'s companion rows bound from `$first`;
/// with `replace`, the companion holds exactly what the model holds. Unchanged when `M` has none.
pub(crate) fn with_companion_write<M: SqlModel>(
    schema: &SchemaName,
    write: &str,
    first: usize,
    replace: bool,
) -> String {
    let Some(companion) = companion::<M>(schema) else {
        return write.to_owned();
    };
    let keys = M::KEY_COLUMNS.join(", ");
    let stored_keys = M::KEY_COLUMNS
        .iter()
        .map(|column| format!("stored.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    let [field, locale, value, hash, translated, by, at] =
        core::array::from_fn(|offset| first + offset);
    let mut sql = format!("WITH stored AS ({write})");
    if replace {
        sql.push_str(&format!(
            ", cleared AS (DELETE FROM {companion} AS i USING stored WHERE {} AND (i.field, \
             i.locale) NOT IN (SELECT u.field, u.locale FROM UNNEST(${field}::text[], \
             ${locale}::text[]) AS u(field, locale)))",
            key_join::<M>("i", "stored"),
        ));
    }
    sql.push_str(&format!(
        ", written AS (INSERT INTO {companion} ({keys}, field, locale, value, source_hash, \
         translation_hash, checked_by, checked_at) SELECT {stored_keys}, u.field, u.locale, \
         u.value, u.source_hash, u.translation_hash, u.checked_by, to_timestamp(u.checked_at / \
         1000.0) FROM stored CROSS JOIN UNNEST(${field}::text[], ${locale}::text[], \
         ${value}::text[], ${hash}::text[], ${translated}::text[], ${by}::text[], \
         ${at}::int8[]) AS u(field, locale, value, source_hash, translation_hash, checked_by, \
         checked_at)"
    ));
    if replace {
        sql.push_str(&format!(
            " ON CONFLICT ({keys}, field, locale) DO UPDATE SET value = excluded.value, \
             source_hash = excluded.source_hash, translation_hash = excluded.translation_hash, \
             checked_by = excluded.checked_by, checked_at = excluded.checked_at"
        ));
    }
    sql.push_str(") SELECT * FROM stored");
    sql
}

/// Binds `model`'s companion rows as the arrays [`with_companion_write`] reads.
pub(crate) fn bind_companion<'q, M: SqlModel>(
    query: Query<'q, Postgres, PgArguments>,
    model: &M,
) -> Query<'q, Postgres, PgArguments> {
    if M::translation_table().is_none() {
        return query;
    }
    let rows = model.translation_rows();
    let mut fields = Vec::with_capacity(rows.len());
    let mut locales = Vec::with_capacity(rows.len());
    let mut values = Vec::with_capacity(rows.len());
    let mut hashes = Vec::with_capacity(rows.len());
    let mut translated = Vec::with_capacity(rows.len());
    let mut by = Vec::with_capacity(rows.len());
    let mut at = Vec::with_capacity(rows.len());
    for row in rows {
        hashes.push(row.check.source_hash().to_owned());
        translated.push(row.check.translation_hash().to_owned());
        by.push(row.check.checked().map(|checked| checked.by().to_owned()));
        at.push(row.check.checked().map(Checked::at_millis));
        fields.push(row.field);
        locales.push(row.locale);
        values.push(row.value);
    }
    query
        .bind(fields)
        .bind(locales)
        .bind(values)
        .bind(hashes)
        .bind(translated)
        .bind(by)
        .bind(at)
}

/// Decodes the row a write of `model` returned and lays `model`'s translations on it.
pub(crate) fn stored_row<M>(row: &PgRow, model: &M, enc: Option<&KeyManager>) -> Result<M, SqlError>
where
    M: SqlModel + FromRow,
{
    let mut stored = M::from_row_encrypted(row, enc)?;
    for row in model.translation_rows() {
        stored.apply_translation(row);
    }
    Ok(stored)
}

/// `delete` extended to delete the companion rows of every row it deletes, answering with the
/// count; `None` when `M` has no translatable fields.
pub(crate) fn with_companion_delete<M: SqlModel>(
    schema: &SchemaName,
    delete: &str,
) -> Option<String> {
    let companion = companion::<M>(schema)?;
    Some(format!(
        "WITH gone AS ({delete} RETURNING {}), companions AS (DELETE FROM {companion} AS i \
         USING gone WHERE {}) SELECT count(*) FROM gone",
        M::KEY_COLUMNS.join(", "),
        key_join::<M>("i", "gone"),
    ))
}

/// Refuses `operation`, a whole-row write without companion rows, for a model with translatable fields.
///
/// # Errors
///
/// Returns a [`SqlError`] naming the model and the operation when `M` has translatable fields.
pub(crate) fn whole_rows_only<M: SqlModel>(operation: &str) -> Result<(), SqlError> {
    match M::translation_table() {
        None => Ok(()),
        Some(companion) => Err(SqlError::query(format!(
            "`{}` keeps text in other languages in `{companion}`, which `{operation}` does not \
             write: write it one row at a time, with insert, update, upsert or insert_if_absent",
            M::TABLE
        ))),
    }
}

/// One companion row as a read selects it beside its record's ([`TRANSLATIONS`]).
type StoredTranslation = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
);

/// What a read of `M` selects from `row`: `columns`, and for a translatable model its companion rows.
pub(crate) fn select_list<M: SqlModel>(schema: &SchemaName, columns: &str, row: &str) -> String {
    let Some(companion) = companion::<M>(schema) else {
        return columns.to_owned();
    };
    format!(
        "{columns}, (SELECT json_agg(json_build_array(i.field, i.locale, i.value, \
         i.source_hash, i.translation_hash, i.checked_by, round(extract(epoch FROM \
         i.checked_at) * 1000)::int8)) FROM {companion} AS i WHERE {}) AS {TRANSLATIONS}",
        key_join::<M>("i", row)
    )
}

/// `SELECT <select_list> FROM <table>`: the start of a read of `M`'s own table.
pub(crate) fn select_from<M: SqlModel>(schema: &SchemaName) -> String {
    let table = schema.qualify(M::TABLE);
    format!(
        "SELECT {} FROM {table}",
        select_list::<M>(schema, "*", &table)
    )
}

/// Decodes `row`, read with [`select_list`], and lays its translations on it.
///
/// # Errors
///
/// Returns a [`SqlError`] if the row cannot be decoded, or if a translatable model's row was read
/// without its translations.
pub(crate) fn decode<M>(row: &PgRow, enc: Option<&KeyManager>) -> Result<M, SqlError>
where
    M: SqlModel + FromRow,
{
    let mut model = M::from_row_encrypted(row, enc)?;
    if M::translation_table().is_some() {
        let translations: Option<sqlx::types::Json<Vec<StoredTranslation>>> =
            row.try_get(TRANSLATIONS)?;
        for (field, locale, value, source_hash, translation_hash, by, at) in
            translations.map(|json| json.0).unwrap_or_default()
        {
            let checked = by.zip(at).map(|(by, at)| Checked::new(by, at));
            model.apply_translation(TranslationRow {
                field,
                locale,
                value,
                check: TranslationCheck::stored(source_hash, translation_hash, checked),
            });
        }
    }
    Ok(model)
}

/// Writes the model's row and its companion rows in one statement, as an insert or an upsert.
async fn persist<M>(
    mut conn: SqlConn<'_>,
    model: &M,
    table: &str,
    upsert: bool,
) -> DatabaseResult<()>
where
    M: SqlModel + ToRow + Send + Sync,
{
    let schema = conn.schema().clone();
    let columns = M::columns();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${i}")).collect();
    let conflict = if upsert {
        let non_pk = M::columns_without_pk();
        let action = if non_pk.is_empty() {
            // A no-op update rather than `DO NOTHING`, so the row is returned for its companion rows.
            let first = M::KEY_COLUMNS[0];
            format!("DO UPDATE SET {first} = excluded.{first}")
        } else {
            let sets: Vec<String> = non_pk
                .iter()
                .map(|c| format!("{c} = excluded.{c}"))
                .collect();
            format!("DO UPDATE SET {}", sets.join(", "))
        };
        format!(" ON CONFLICT ({}) {}", M::KEY_COLUMNS.join(", "), action)
    } else {
        String::new()
    };
    let write = format!(
        "INSERT INTO {} ({}) VALUES ({}){} RETURNING *",
        schema.qualify(table),
        columns.join(", "),
        placeholders.join(", "),
        conflict
    );
    let sql = with_companion_write::<M>(&schema, &write, columns.len() + 1, upsert);
    let query = bind_companion(model.bind_to(pg_query(sql.clone())), model);
    conn.execute_raw(query)
        .await
        .map_err(|e| DatabaseError::Query {
            message: e.to_string(),
            query: Some(sql),
        })?;
    Ok(())
}

/// Inserts a translatable model and its companion rows in one statement; called by the generated `Model::insert`.
#[instrument(skip(conn, model), fields(model = M::TYPE_NAME), err(Debug))]
#[expect(clippy::missing_errors_doc, reason = "readability baseline")]
pub async fn insert_translated_model<'a, M>(
    conn: SqlConn<'a>,
    model: &M,
    table: &str,
) -> DatabaseResult<()>
where
    M: SqlModel + ToRow + Send + Sync,
{
    persist(conn, model, table, false).await
}

/// Inserts or updates a translatable model and its companion rows in one statement; called by the generated `Model::upsert`.
#[instrument(skip(conn, model), fields(model = M::TYPE_NAME), err(Debug))]
#[expect(clippy::missing_errors_doc, reason = "readability baseline")]
pub async fn upsert_translated_model<'a, M>(
    conn: SqlConn<'a>,
    model: &M,
    table: &str,
) -> DatabaseResult<()>
where
    M: SqlModel + ToRow + Send + Sync,
{
    persist(conn, model, table, true).await
}

/// Finds the keys of a single-column-key model whose translatable `column` matches `pattern` (`ILIKE`) in `locale`.
#[instrument(level = "debug", skip(conn, pattern), fields(model = M::TYPE_NAME, column, locale))]
#[expect(clippy::missing_errors_doc, reason = "readability baseline")]
pub async fn find_translation_ids<'a, M>(
    conn: SqlConn<'a>,
    column: &str,
    locale: &str,
    pattern: &str,
) -> DatabaseResult<Vec<M::Id>>
where
    M: SqlModel,
    M::Id: Send + Unpin + for<'r> Decode<'r, Postgres> + Type<Postgres>,
{
    let mut conn = conn;
    let Some(table) = companion::<M>(conn.schema()) else {
        return Ok(Vec::new());
    };
    let key_column = M::PRIMARY_KEY;
    crate::ident::validate_identifier(key_column, false)
        .map_err(|e| DatabaseError::query(e.to_string()))?;
    let canonical = locale
        .parse::<Locale>()
        .map_or_else(|_| locale.to_string(), |l| l.to_string());
    let sql = format!(
        "SELECT DISTINCT {key_column} FROM {table} \
         WHERE field = $1 AND locale = $2 AND value ILIKE $3"
    );
    let query = pg_query(sql)
        .bind(column.to_string())
        .bind(canonical)
        .bind(pattern.to_string());
    let rows = conn.fetch_all_raw(query).await?;
    rows.iter()
        .map(|r| {
            r.try_get::<M::Id, _>(key_column)
                .map_err(|e| DatabaseError::serialization(e.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::model::{BoundTo, Model, SqlDb};

    struct Notice;
    impl Model for Notice {
        const TYPE_NAME: &'static str = "Notice";
    }
    impl BoundTo<SqlDb> for Notice {}
    impl SqlModel for Notice {
        type Id = ();
        fn id(&self) -> &Self::Id {
            &()
        }
        const TABLE: &'static str = "notices";
        const KEY_COLUMNS: &'static [&'static str] = &["organisation_id", "id"];
        fn translation_table() -> Option<&'static str> {
            Some("notices_i18n")
        }
    }

    #[test]
    fn a_read_selects_each_rows_translations_beside_it_joined_on_the_whole_key() {
        let schema = SchemaName::new("s").expect("a plain identifier");
        assert_eq!(
            select_from::<Notice>(&schema),
            "SELECT *, (SELECT json_agg(json_build_array(i.field, i.locale, i.value, \
             i.source_hash, i.translation_hash, i.checked_by, round(extract(epoch FROM \
             i.checked_at) * 1000)::int8)) FROM s.notices_i18n AS i WHERE \
             i.organisation_id = s.notices.organisation_id AND i.id = s.notices.id) AS fundamental_translations \
             FROM s.notices"
        );
    }

    #[test]
    fn a_delete_takes_the_companion_rows_of_the_rows_it_deleted() {
        let sql = with_companion_delete::<Notice>(
            &SchemaName::default(),
            "DELETE FROM notices WHERE organisation_id = $1",
        )
        .expect("a translatable model");
        assert!(
            sql.contains("RETURNING organisation_id, id")
                && sql.contains(
                    "USING gone WHERE i.organisation_id = gone.organisation_id AND i.id = gone.id"
                ),
            "{sql}"
        );
    }

    #[test]
    fn a_write_that_cannot_carry_translations_refuses_a_translatable_model() {
        let refused = whole_rows_only::<Notice>("insert_many")
            .expect_err("a batch would store the source alone");
        assert!(
            refused.to_string().contains("notices") && refused.to_string().contains("insert_many"),
            "{refused}"
        );
    }
}
