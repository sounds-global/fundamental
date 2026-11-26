//! In-memory evaluation of a slice [`Filter`] against a [`SyncRecord`].

use fundamental_db::{Filter, FilterCondition, FilterValue};
use tracing::warn;

use crate::SyncRecord;

/// Returns whether `record` satisfies `filter`, evaluated in memory over the payload's text.
#[must_use]
pub fn record_matches(filter: &Filter, record: &SyncRecord, key_columns: &[&str]) -> bool {
    eval(&filter.condition, record, key_columns)
}

fn eval(condition: &FilterCondition, record: &SyncRecord, keys: &[&str]) -> bool {
    match condition {
        FilterCondition::Eq { column, value } => text_eq(record, keys, column, value),
        FilterCondition::Ne { column, value } => !text_eq(record, keys, column, value),
        FilterCondition::Gt { column, value } => num_cmp(record, keys, column, value, |a, b| a > b),
        FilterCondition::Gte { column, value } => {
            num_cmp(record, keys, column, value, |a, b| a >= b)
        }
        FilterCondition::Lt { column, value } => num_cmp(record, keys, column, value, |a, b| a < b),
        FilterCondition::Lte { column, value } => {
            num_cmp(record, keys, column, value, |a, b| a <= b)
        }
        FilterCondition::In { column, values } => {
            values.iter().any(|v| text_eq(record, keys, column, v))
        }
        FilterCondition::NotIn { column, values } => {
            !values.iter().any(|v| text_eq(record, keys, column, v))
        }
        FilterCondition::IsNull { column } => {
            column_value(record, keys, column).is_none_or(str::is_empty)
        }
        FilterCondition::IsNotNull { column } => {
            column_value(record, keys, column).is_some_and(|s| !s.is_empty())
        }
        FilterCondition::Like { column, pattern } => like(record, keys, column, pattern, true),
        FilterCondition::ILike { column, pattern } => like(record, keys, column, pattern, false),
        FilterCondition::Between { column, low, high } => {
            num_cmp(record, keys, column, low, |a, b| a >= b)
                && num_cmp(record, keys, column, high, |a, b| a <= b)
        }
        FilterCondition::And(filters) => filters.iter().all(|f| eval(&f.condition, record, keys)),
        FilterCondition::Or(filters) => filters.iter().any(|f| eval(&f.condition, record, keys)),
        // A row value is a cursor boundary, not a membership rule: fail closed.
        FilterCondition::RowValueGt { columns, .. }
        | FilterCondition::RowValueLt { columns, .. } => {
            warn!(
                columns = ?columns,
                "a slice filter carries a row-value comparison; that is a pagination \
                 boundary rather than a membership rule, and no record can match it"
            );
            false
        }
        FilterCondition::Not(filter) => !eval(&filter.condition, record, keys),
    }
}

/// The text value of `column`, from the record's key when it addresses the row and from its
/// payload otherwise, so a payload cannot override the key a push is authorised by.
fn column_value<'a>(record: &'a SyncRecord, keys: &[&str], column: &str) -> Option<&'a str> {
    match keys.iter().position(|c| *c == column) {
        Some(at) => record.id.components().nth(at),
        None => record.payload.get(column).map(String::as_str),
    }
}

/// Renders a filter value to the payload's text form, or `None` when it has none.
fn render(value: &FilterValue) -> Option<String> {
    match value {
        FilterValue::Bool(v) => Some(v.to_string()),
        FilterValue::Int(v) => Some(v.to_string()),
        FilterValue::Float(v) => Some(v.to_string()),
        FilterValue::String(v) | FilterValue::Uuid(v) => Some(v.clone()),
        FilterValue::Timestamp(v) => Some(v.to_string()),
        FilterValue::Null
        | FilterValue::Bytes(_)
        | FilterValue::TimestampTz(_)
        | FilterValue::Json(_) => None,
    }
}

fn text_eq(record: &SyncRecord, keys: &[&str], column: &str, value: &FilterValue) -> bool {
    match render(value) {
        Some(text) => column_value(record, keys, column) == Some(text.as_str()),
        None => false,
    }
}

fn as_f64(value: &FilterValue) -> Option<f64> {
    match value {
        FilterValue::Int(v) => Some(*v as f64),
        FilterValue::Float(v) => Some(*v),
        FilterValue::Timestamp(v) => Some(*v as f64),
        FilterValue::String(v) => v.parse().ok(),
        _ => None,
    }
}

fn num_cmp(
    record: &SyncRecord,
    keys: &[&str],
    column: &str,
    value: &FilterValue,
    op: impl Fn(f64, f64) -> bool,
) -> bool {
    match (
        column_value(record, keys, column).and_then(|s| s.parse::<f64>().ok()),
        as_f64(value),
    ) {
        (Some(lhs), Some(rhs)) => op(lhs, rhs),
        _ => false,
    }
}

fn like(
    record: &SyncRecord,
    keys: &[&str],
    column: &str,
    pattern: &str,
    case_sensitive: bool,
) -> bool {
    let Some(text) = column_value(record, keys, column) else {
        return false;
    };
    if case_sensitive {
        like_match(pattern, text)
    } else {
        like_match(&pattern.to_lowercase(), &text.to_lowercase())
    }
}

/// Matches a SQL `LIKE` pattern (`%` any sequence, `_` any single char) against `text`.
fn like_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '_' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '%' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '%' {
        pi += 1;
    }
    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyncMeta;
    use fundamental_db::RecordKey;
    use std::collections::BTreeMap;

    fn record(pairs: &[(&str, &str)]) -> SyncRecord {
        let mut payload = BTreeMap::new();
        for (k, v) in pairs {
            payload.insert((*k).to_string(), (*v).to_string());
        }
        SyncRecord {
            id: "r".into(),
            meta: SyncMeta::new(1, 1),
            payload,
        }
    }

    #[test]
    fn a_key_column_is_read_from_the_key_and_a_payload_cannot_override_it() {
        let keys = ["owner_id", "row_id"];
        let mut payload = BTreeMap::new();
        payload.insert("owner_id".to_string(), "bob".to_string());
        payload.insert("ciphertext".to_string(), "sealed".to_string());
        let record = SyncRecord {
            id: RecordKey::composite("alice", ["row-1"]),
            meta: SyncMeta::new(1, 1),
            payload,
        };

        assert!(
            record_matches(&Filter::eq("owner_id", "alice"), &record, &keys),
            "the key's owner is what the row is measured against"
        );
        assert!(
            !record_matches(&Filter::eq("owner_id", "bob"), &record, &keys),
            "a payload `owner_id` must not authorise a row the key does not address"
        );
        assert!(record_matches(
            &Filter::eq("row_id", "row-1"),
            &record,
            &keys
        ));
        assert!(!record_matches(
            &Filter::eq("row_id", "alice"),
            &record,
            &keys
        ));
        assert!(record_matches(
            &Filter::eq("ciphertext", "sealed"),
            &record,
            &keys
        ));
    }

    #[test]
    fn equality_and_membership() {
        let r = record(&[("owner_id", "alice"), ("age", "30")]);
        assert!(record_matches(&Filter::eq("owner_id", "alice"), &r, &[]));
        assert!(!record_matches(&Filter::eq("owner_id", "bob"), &r, &[]));
        assert!(record_matches(&Filter::ne("owner_id", "bob"), &r, &[]));
        assert!(record_matches(
            &Filter::in_list("owner_id", vec!["bob".into(), "alice".into()]),
            &r,
            &[],
        ));
        assert!(!record_matches(
            &Filter::in_list("owner_id", vec!["bob".into(), "carol".into()]),
            &r,
            &[],
        ));
    }

    #[test]
    fn numeric_comparisons() {
        let r = record(&[("age", "30")]);
        assert!(record_matches(&Filter::gt("age", 18), &r, &[]));
        assert!(!record_matches(&Filter::gt("age", 30), &r, &[]));
        assert!(record_matches(&Filter::gte("age", 30), &r, &[]));
        assert!(record_matches(&Filter::between("age", 20, 40), &r, &[]));
        assert!(!record_matches(&Filter::between("age", 0, 10), &r, &[]));
    }

    #[test]
    fn null_checks_and_missing_columns() {
        let r = record(&[("name", ""), ("owner_id", "alice")]);
        assert!(record_matches(&Filter::is_null("name"), &r, &[]));
        assert!(record_matches(&Filter::is_null("absent"), &r, &[]));
        assert!(record_matches(&Filter::is_not_null("owner_id"), &r, &[]));
        assert!(!record_matches(&Filter::is_not_null("name"), &r, &[]));
    }

    #[test]
    fn boolean_composition() {
        let r = record(&[("owner_id", "alice"), ("age", "30")]);
        let mine_and_adult = Filter::and(vec![
            Filter::eq("owner_id", "alice"),
            Filter::gte("age", 18),
        ]);
        assert!(record_matches(&mine_and_adult, &r, &[]));

        let bob_or_adult = Filter::or(vec![Filter::eq("owner_id", "bob"), Filter::gte("age", 18)]);
        assert!(record_matches(&bob_or_adult, &r, &[]));

        assert!(!record_matches(
            &Filter::not(Filter::eq("owner_id", "alice")),
            &r,
            &[],
        ));
    }

    #[test]
    fn like_patterns() {
        let r = record(&[("name", "Cataract")]);
        assert!(record_matches(&Filter::like("name", "Cat%"), &r, &[]));
        assert!(record_matches(&Filter::like("name", "%ract"), &r, &[]));
        assert!(record_matches(&Filter::like("name", "%tara%"), &r, &[]));
        assert!(record_matches(&Filter::like("name", "Cat_ract"), &r, &[]));
        assert!(!record_matches(&Filter::like("name", "Dog%"), &r, &[]));
        assert!(record_matches(&Filter::ilike("name", "cat%"), &r, &[]));
    }
}
