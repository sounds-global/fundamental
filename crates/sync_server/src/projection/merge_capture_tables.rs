use std::collections::HashMap;

use fundamental_sync::CaptureTable;

/// Unions capture-table specs from several sources into one set with a single entry
/// per table; the first spec for a table wins.
#[must_use]
pub fn merge_capture_tables(tables: impl IntoIterator<Item = CaptureTable>) -> Vec<CaptureTable> {
    let mut seen = HashMap::new();
    for table in tables {
        seen.entry(table.table().to_string()).or_insert(table);
    }
    seen.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_dedups_by_table_name() {
        let a = CaptureTable::new("messages", ["id"], ["id"], ["channel_id"], "channel_id");
        let b = CaptureTable::new(
            "channel_summaries",
            ["id"],
            ["id"],
            ["channel_id"],
            "channel_id",
        );
        let a_again = CaptureTable::new("messages", ["id"], ["id"], ["channel_id"], "channel_id");

        let mut merged = merge_capture_tables([a, b, a_again]);
        merged.sort_by(|x, y| x.table().cmp(y.table()));

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].table(), "channel_summaries");
        assert_eq!(merged[1].table(), "messages");
    }
}
