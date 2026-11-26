//! A page's translations, read in one query and matched back to its records.
//!
//! A page comes from one partition, which the companion shares, so one bounded read holds every
//! translation of the page.

use std::collections::HashMap;

use fundamental_core::i18n::{Checked, TranslationCheck, TranslationRow};
use fundamental_core::model::CqlModel;
use fundamental_core::model::cql::ClusteringOrder;
use scylla::serialize::value::SerializeValue;
use scylla::value::{CqlValue as WireValue, Row};

use super::{WireKey, qualify};
use crate::{CqlError, CqlResult, CqlSession, CqlValue, PreparedCache, ToRow};

/// Hydrates every record of a page with its translations, in one companion read.
///
/// `partition` holds the values the page was read by; each record comes with its clustering
/// key as the page's row returned it. A no-op for a model without translatable fields or an
/// empty page.
pub(crate) async fn hydrate_page<M: CqlModel + ToRow>(
    session: &CqlSession,
    cache: &PreparedCache,
    keyspace: &str,
    partition: &[CqlValue],
    page: Vec<(WireKey, M)>,
) -> CqlResult<Vec<M>> {
    let ttable = match M::translation_table() {
        Some(ttable) if !page.is_empty() => ttable,
        _ => return Ok(page.into_iter().map(|(_, model)| model).collect()),
    };
    let clustering = M::clustering_key_columns();
    let mut condition: Vec<String> = M::partition_key_columns()
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect();
    // `Sync`, so the page's future stays `Send` for callers on a multi-threaded runtime.
    let mut values: Vec<&(dyn SerializeValue + Sync)> = partition
        .iter()
        .map(|v| v as &(dyn SerializeValue + Sync))
        .collect();
    let range = range(&page, M::clustering_key_orders());
    if let (Some((low, high)), Some(first)) = (&range, clustering.first()) {
        condition.push(format!("{first} >= ? AND {first} <= ?"));
        values.push(low);
        values.push(high);
    }
    let selected = clustering
        .iter()
        .map(|column| format!("{column}, "))
        .collect::<String>();
    let cql = format!(
        "SELECT {selected}field, locale, value, source_hash, translation_hash, checked_by, \
         checked_at FROM {} WHERE {}",
        qualify(keyspace, ttable),
        condition.join(" AND "),
    );
    let stmt = cache.get_or_prepare(&cql).await?;
    let result = session
        .inner()
        .execute_unpaged(&stmt, &values)
        .await
        .map_err(|e| CqlError::Query(Box::new(e)))?;
    let rows = result
        .into_rows_result()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?;
    let decoded = rows
        .rows::<Row>()
        .map_err(|e| CqlError::RowConversion(e.to_string()))?
        .map(|row| {
            let row = row.map_err(|e| CqlError::RowConversion(e.to_string()))?;
            decode(row, clustering.len())
        })
        .collect::<CqlResult<Vec<_>>>()?;
    Ok(apply(page, group(decoded)))
}

/// The low and high value of the page's first clustering column: its first and last row's,
/// swapped when that column is stored descending. `None` for a model without clustering
/// columns, whose partition holds one record, or one whose order is not declared.
fn range<M>(page: &[(WireKey, M)], orders: &[ClusteringOrder]) -> Option<(WireValue, WireValue)> {
    let order = orders.first()?;
    let first = page.first()?.0.first()?.clone();
    let last = page.last()?.0.first()?.clone();
    Some(match order {
        ClusteringOrder::Asc => (first, last),
        ClusteringOrder::Desc => (last, first),
    })
}

/// One companion row: its clustering key, then the translation.
fn decode(row: Row, key_len: usize) -> CqlResult<(WireKey, TranslationRow)> {
    let mut columns = row.columns;
    if columns.len() != key_len + 7 {
        return Err(CqlError::RowConversion(format!(
            "a translation row has {} columns, expected {}",
            columns.len(),
            key_len + 7
        )));
    }
    let rest = columns.split_off(key_len);
    let mut rest = rest.into_iter();
    let mut text = || rest.next().flatten();
    let field = required(text(), "field")?;
    let locale = required(text(), "locale")?;
    let value = required(text(), "value")?;
    let source_hash = text().and_then(WireValue::into_string);
    let translation_hash = text().and_then(WireValue::into_string);
    let by = text().and_then(WireValue::into_string);
    let at = text().and_then(|v| v.as_bigint());
    Ok((
        WireKey(columns),
        TranslationRow {
            field,
            locale,
            value,
            check: TranslationCheck::stored(
                source_hash.unwrap_or_default(),
                translation_hash.unwrap_or_default(),
                by.zip(at).map(|(by, at)| Checked::new(by, at)),
            ),
        },
    ))
}

/// A column every translation row holds, as text.
fn required(value: Option<WireValue>, column: &str) -> CqlResult<String> {
    value
        .and_then(WireValue::into_string)
        .ok_or_else(|| CqlError::RowConversion(format!("a translation row has no {column}")))
}

/// The translations grouped by the record they belong to, each group in the order read.
pub(crate) fn group(
    rows: impl IntoIterator<Item = (WireKey, TranslationRow)>,
) -> HashMap<WireKey, Vec<TranslationRow>> {
    let mut groups: HashMap<WireKey, Vec<TranslationRow>> = HashMap::new();
    for (key, row) in rows {
        groups.entry(key).or_default().push(row);
    }
    groups
}

/// Puts each group on its record. A page holds each key once, so a group is moved, not
/// cloned.
fn apply<M: CqlModel>(
    page: Vec<(WireKey, M)>,
    mut groups: HashMap<WireKey, Vec<TranslationRow>>,
) -> Vec<M> {
    page.into_iter()
        .map(|(key, mut model)| {
            for row in groups.remove(&key).into_iter().flatten() {
                model.apply_translation(row);
            }
            model
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: i64) -> WireKey {
        WireKey(vec![Some(WireValue::BigInt(id))])
    }

    fn translation(field: &str, locale: &str, value: &str) -> TranslationRow {
        TranslationRow {
            field: field.into(),
            locale: locale.into(),
            value: value.into(),
            check: TranslationCheck::stored(String::new(), String::new(), None),
        }
    }

    /// Compiled, never run: a page's read can be awaited on a multi-threaded runtime.
    #[expect(dead_code, reason = "a compile-time check, never called")]
    fn a_page_is_read_from_any_thread<'a, M: CqlModel + ToRow + Send + 'a>(
        session: &'a CqlSession,
        cache: &'a PreparedCache,
        partition: &'a [CqlValue],
    ) -> impl Send + 'a {
        hydrate_page::<M>(session, cache, "keyspace", partition, Vec::new())
    }

    #[test]
    fn a_descending_page_is_bounded_from_its_last_row_to_its_first() {
        let keys = [(key(9), ()), (key(5), ()), (key(2), ())];

        assert_eq!(
            range(&keys, &[ClusteringOrder::Desc]),
            Some((WireValue::BigInt(2), WireValue::BigInt(9)))
        );
        assert_eq!(
            range(&keys, &[ClusteringOrder::Asc]),
            Some((WireValue::BigInt(9), WireValue::BigInt(2)))
        );
        assert_eq!(range(&keys, &[]), None, "an undeclared order reads it all");
        assert_eq!(range::<()>(&[], &[ClusteringOrder::Asc]), None);
    }

    #[test]
    fn a_companion_row_is_read_as_its_key_and_its_translation() {
        let row = Row {
            columns: vec![
                Some(WireValue::BigInt(5)),
                Some(WireValue::Text("title".into())),
                Some(WireValue::Text("nl".into())),
                Some(WireValue::Text("Nieuws".into())),
                Some(WireValue::Text("src".into())),
                Some(WireValue::Text("tr".into())),
                Some(WireValue::Text("ana".into())),
                Some(WireValue::BigInt(1_700)),
            ],
        };

        let (owner, read) = decode(row, 1).expect("a translation row");

        assert_eq!(owner, key(5));
        assert_eq!(
            (
                read.field.as_str(),
                read.locale.as_str(),
                read.value.as_str()
            ),
            ("title", "nl", "Nieuws")
        );
        assert_eq!(read.check.source_hash(), "src");
        assert_eq!(read.check.checked().map(Checked::by), Some("ana"));
    }

    #[test]
    fn a_row_missing_columns_is_refused() {
        let row = Row {
            columns: vec![Some(WireValue::BigInt(5)), None],
        };
        assert!(matches!(decode(row, 1), Err(CqlError::RowConversion(_))));
    }

    #[test]
    fn translations_are_grouped_by_their_record_in_the_order_read() {
        let groups = group([
            (key(1), translation("title", "de", "Nachrichten")),
            (key(2), translation("title", "nl", "Sport")),
            (key(1), translation("title", "nl", "Nieuws")),
        ]);

        let locales = |k| {
            groups[&k]
                .iter()
                .map(|r| r.locale.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(locales(key(1)), ["de", "nl"]);
        assert_eq!(locales(key(2)), ["nl"]);
        assert!(!groups.contains_key(&key(3)));
    }
}
