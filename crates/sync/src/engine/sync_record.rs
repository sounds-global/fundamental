//! A single row moving through the sync engine.

use std::collections::BTreeMap;
use std::str::FromStr;

use fundamental_db::RecordKey;

use crate::engine::SyncMeta;

/// A row as the sync engine sees it: a key, its sync [metadata](SyncMeta), and an opaque payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRecord {
    /// The row's key columns, in key order, each rendered as text.
    pub id: RecordKey,
    /// Conflict-resolution metadata (the overlay columns).
    pub meta: SyncMeta,
    /// The synced data columns: name to rendered value.
    pub payload: BTreeMap<String, String>,
}

impl SyncRecord {
    /// Creates a record from its identity, metadata, and payload.
    pub fn new(
        id: impl Into<RecordKey>,
        meta: SyncMeta,
        payload: BTreeMap<String, String>,
    ) -> Self {
        Self {
            id: id.into(),
            meta,
            payload,
        }
    }

    /// The raw text of payload `column`, or `None` if the record does not carry it.
    #[must_use]
    pub fn text(&self, column: &str) -> Option<&str> {
        self.payload.get(column).map(String::as_str)
    }

    /// Payload `column` parsed as `T`, or `None` if absent or unparsable.
    #[must_use]
    pub fn parse<T: FromStr>(&self, column: &str) -> Option<T> {
        self.text(column)?.trim().parse().ok()
    }

    /// Payload `column` as a boolean (`true`/`false`, `t`/`f`, `1`/`0`, any case), or `None`.
    #[must_use]
    pub fn bool(&self, column: &str) -> Option<bool> {
        match self.text(column)?.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" => Some(true),
            "false" | "f" | "0" => Some(false),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: &[(&str, &str)]) -> SyncRecord {
        let payload = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        SyncRecord::new("id-1", SyncMeta::new(1, 1), payload)
    }

    #[test]
    fn text_returns_the_raw_column_or_none() {
        let record = record(&[("title", "hello")]);
        assert_eq!(record.text("title"), Some("hello"));
        assert_eq!(record.text("missing"), None);
    }

    #[test]
    fn parse_reads_a_numeric_column_and_none_on_junk() {
        let record = record(&[("total", "42"), ("bad", "not-a-number")]);
        assert_eq!(record.parse::<i64>("total"), Some(42));
        assert_eq!(record.parse::<i64>("bad"), None);
        assert_eq!(record.parse::<i64>("missing"), None);
    }

    #[test]
    fn bool_accepts_every_synced_text_form() {
        for truthy in ["true", "TRUE", "t", "1", " true "] {
            assert_eq!(
                record(&[("done", truthy)]).bool("done"),
                Some(true),
                "{truthy}"
            );
        }
        for falsy in ["false", "F", "0", " false "] {
            assert_eq!(
                record(&[("done", falsy)]).bool("done"),
                Some(false),
                "{falsy}"
            );
        }
    }

    #[test]
    fn bool_is_none_for_a_non_boolean_or_missing_column() {
        assert_eq!(record(&[("done", "yes")]).bool("done"), None);
        assert_eq!(record(&[]).bool("done"), None);
    }
}
