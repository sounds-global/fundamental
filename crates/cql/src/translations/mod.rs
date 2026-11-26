//! Companion-table persistence for translatable (`Localized<…>`) model fields.
//!
//! Other locales live in a `<table>_i18n` companion keyed by the record's own key, in its
//! partition; there is no cross-table transaction, so the companion writes are best-effort.

mod chained;
mod page;
mod wire_key;

use scylla::deserialize::row::DeserializeRow;
use scylla::statement::batch::{Batch, BatchType};

use crate::CqlSession;

use fundamental_core::i18n::{Checked, TranslationCheck, TranslationRow};
use fundamental_core::model::CqlModel;

use crate::{CqlError, CqlResult, CqlValue, CqlValues, PreparedCache, ToRow};

use chained::Chained;
#[cfg(feature = "internal-benches")]
pub(crate) use page::group;
pub(crate) use page::hydrate_page;
pub(crate) use wire_key::WireKey;

/// Qualifies a table name with the keyspace when one is set.
fn qualify(keyspace: &str, table: &str) -> String {
    if keyspace.is_empty() {
        table.to_string()
    } else {
        format!("{keyspace}.{table}")
    }
}

/// The record's key columns, partition then clustering: the order its
/// [`ToRow::KeyValues`] binds in, and the companion's leading columns.
fn key_columns<M: ToRow>() -> Vec<&'static str> {
    let mut columns = M::partition_key_columns().to_vec();
    columns.extend_from_slice(M::clustering_key_columns());
    columns
}

/// `pk = ? AND ck = ?`, over the record's key columns.
fn key_condition<M: ToRow>() -> String {
    key_columns::<M>()
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// (Re)writes a model's companion translation rows — the existing overrides for
/// the record are cleared first, so a locale dropped from the model disappears.
/// A no-op for non-translatable models.
pub(crate) async fn write_translations<M: CqlModel + ToRow>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    model: &M,
) -> CqlResult<()> {
    if M::translation_table().is_none() {
        return Ok(());
    }
    write_rows::<M>(
        session,
        cache,
        keyspace,
        &model.to_key_values(),
        model.translation_rows(),
    )
    .await
}

/// (Re)writes the companion rows of the record keyed `key`: clears its overrides, then
/// writes `rows` in one batch, all in the record's partition. A no-op for non-translatable
/// models.
pub(crate) async fn write_rows<M: CqlModel + ToRow>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    key: &M::KeyValues,
    rows: Vec<TranslationRow>,
) -> CqlResult<()> {
    let Some(ttable) = M::translation_table() else {
        return Ok(());
    };
    // The clear runs on its own, before the writes: in one batch the two would share a
    // write time, and the delete would win over the rows written after it.
    delete_translations::<M>(session, cache, keyspace, key).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let columns = key_columns::<M>();
    let markers = vec!["?"; columns.len() + 7].join(", ");
    let cql = format!(
        "INSERT INTO {} ({}, field, locale, value, source_hash, translation_hash, checked_by, \
         checked_at) VALUES ({markers})",
        qualify(keyspace, ttable),
        columns.join(", "),
    );
    let stmt = cache.get_or_prepare(&cql).await?;
    let values: Vec<Chained<'_, M::KeyValues>> = rows
        .into_iter()
        .map(|row| Chained(key, companion_values(row)))
        .collect();
    let mut batch = Batch::new(BatchType::Logged);
    for _ in &values {
        batch.append_statement((*stmt).clone());
    }
    session
        .inner()
        .batch(&batch, &values)
        .await
        .map_err(|e| CqlError::Query(Box::new(e)))?;
    Ok(())
}

/// A companion row's values after the record's key, in the order the `INSERT` names them.
fn companion_values(row: TranslationRow) -> CqlValues {
    let checked = row.check.checked();
    CqlValues(vec![
        CqlValue::text(row.field),
        CqlValue::text(row.locale),
        CqlValue::text(row.value),
        CqlValue::text(row.check.source_hash()),
        CqlValue::text(row.check.translation_hash()),
        checked.map_or(CqlValue::Null, |checked| CqlValue::text(checked.by())),
        checked.map_or(CqlValue::Null, |checked| {
            CqlValue::bigint(checked.at_millis())
        }),
    ])
}

/// One companion row as a read selects it.
type StoredTranslation = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

/// Hydrates one loaded model's translatable fields from the companion table, by its key.
pub(crate) async fn hydrate<M: CqlModel + ToRow>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    model: &mut M,
) -> CqlResult<()> {
    let Some(ttable) = M::translation_table() else {
        return Ok(());
    };
    let cql = format!(
        "SELECT field, locale, value, source_hash, translation_hash, checked_by, checked_at FROM \
         {} WHERE {}",
        qualify(keyspace, ttable),
        key_condition::<M>(),
    );
    let stmt = cache.get_or_prepare(&cql).await?;
    let result = session
        .inner()
        .execute_unpaged(&stmt, &model.to_key_values())
        .await
        .map_err(|e| CqlError::Query(Box::new(e)))?;

    let rows_result = result
        .into_rows_result()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?;
    let rows = rows_result
        .rows::<StoredTranslation>()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?;
    for row in rows {
        let (field, locale, value, source_hash, translation_hash, by, at) =
            row.map_err(|e| CqlError::RowConversion(e.to_string()))?;
        model.apply_translation(TranslationRow {
            field,
            locale,
            value,
            check: TranslationCheck::stored(
                source_hash.unwrap_or_default(),
                translation_hash.unwrap_or_default(),
                by.zip(at).map(|(by, at)| Checked::new(by, at)),
            ),
        });
    }
    Ok(())
}

/// Finds the keys of the records whose translatable `column` exactly equals `value` in
/// `locale`; YCQL has no `LIKE`, so this uses `ALLOW FILTERING`.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) async fn find_keys<M>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    column: &str,
    locale: &str,
    value: &str,
) -> CqlResult<Vec<M::KeyValues>>
where
    M: CqlModel + ToRow,
    M::KeyValues: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
{
    let Some(ttable) = M::translation_table() else {
        return Ok(Vec::new());
    };
    let canonical = locale
        .parse::<fundamental_core::i18n::Locale>()
        .map_or_else(|_| locale.to_string(), |l| l.to_string());
    let cql = format!(
        "SELECT {} FROM {} WHERE field = ? AND locale = ? AND value = ? ALLOW FILTERING",
        key_columns::<M>().join(", "),
        qualify(keyspace, ttable)
    );
    let stmt = cache.get_or_prepare(&cql).await?;
    let values = CqlValues(vec![
        CqlValue::text(column),
        CqlValue::text(canonical),
        CqlValue::text(value),
    ]);
    let result = session
        .inner()
        .execute_unpaged(&stmt, &values)
        .await
        .map_err(|e| CqlError::Query(Box::new(e)))?;
    let rows_result = result
        .into_rows_result()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?;
    rows_result
        .rows::<M::KeyValues>()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?
        .map(|row| row.map_err(|e| CqlError::RowConversion(e.to_string())))
        .collect()
}

/// Deletes the companion translation rows of the record keyed `key`. A no-op for
/// non-translatable models.
pub(crate) async fn delete_translations<M: CqlModel + ToRow>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    key: &M::KeyValues,
) -> CqlResult<()> {
    let Some(ttable) = M::translation_table() else {
        return Ok(());
    };
    let cql = format!(
        "DELETE FROM {} WHERE {}",
        qualify(keyspace, ttable),
        key_condition::<M>()
    );
    let stmt = cache.get_or_prepare(&cql).await?;
    session
        .inner()
        .execute_unpaged(&stmt, key)
        .await
        .map_err(|e| CqlError::Query(Box::new(e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::CqlDb;
    use fundamental_core::model::cql::ClusteringOrder;
    use fundamental_db::migration::ModelSchema;
    use fundamental_macros::Model;

    /// A notice of one organisation, newest first, with a title in several languages.
    #[derive(Clone, Debug, PartialEq, Model)]
    #[model(cql, table = "notices")]
    struct Notice {
        #[model(partition_key)]
        organisation_id: uuid::Uuid,
        #[model(clustering_key = "desc")]
        id: i64,
        title: fundamental_core::i18n::Localized<String>,
    }

    #[test]
    fn a_derived_model_says_the_order_of_its_clustering_columns() {
        use crate::ToRow;

        assert_eq!(Notice::clustering_key_orders(), [ClusteringOrder::Desc]);
    }

    #[test]
    fn a_localized_models_companion_shares_its_partition() {
        let companion = <Notice as ModelSchema<CqlDb>>::translation_schema()
            .expect("a model with localized text has a companion");

        assert_eq!(companion.name, "notices_i18n");
        assert_eq!(
            companion.partition_key.as_deref(),
            Some(&["organisation_id".to_owned()][..])
        );
        assert_eq!(
            companion.clustering_key.as_deref(),
            Some(
                &[
                    ("id".to_owned(), ClusteringOrder::Desc),
                    ("field".to_owned(), ClusteringOrder::Asc),
                    ("locale".to_owned(), ClusteringOrder::Asc),
                ][..]
            )
        );
    }
}
