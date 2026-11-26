//! [`SliceCursor`] — a keyset continuation point for paged slice pulls.

use fundamental_db::RecordKey;
use serde::{Deserialize, Serialize};

/// A keyset continuation point on `(updated_at, key)`: the last row a page delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCursor {
    /// The last delivered row's overlay `updated_at` in milliseconds.
    pub updated_at: i64,
    /// The last delivered row's key columns, in key order.
    pub key: RecordKey,
}

impl SliceCursor {
    /// Creates a cursor at `(updated_at, key)`.
    pub fn new(updated_at: i64, key: impl Into<RecordKey>) -> Self {
        Self {
            updated_at,
            key: key.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_column_cursor_round_trips_on_the_wire() {
        let cursor = SliceCursor::new(10, "row-1");
        let json = serde_json::to_string(&cursor).expect("serialize");
        assert!(
            json.contains(r#""key":"row-1""#),
            "a single-column key stays a bare string: {json}"
        );
        let back: SliceCursor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cursor);
    }

    #[test]
    fn a_composite_cursor_round_trips_on_the_wire() {
        let cursor = SliceCursor::new(10, RecordKey::composite("eu", ["owner-1", "row-1"]));
        let json = serde_json::to_string(&cursor).expect("serialize");
        let back: SliceCursor = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cursor);
    }
}
