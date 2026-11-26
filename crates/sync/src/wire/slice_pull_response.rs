//! [`SlicePullResponse`] — one page of a slice pull.

use serde::{Deserialize, Serialize};

use crate::wire::{SliceCursor, WireRecord};

/// One page of changed rows in response to a
/// [`SlicePullRequest`](crate::wire::SlicePullRequest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlicePullResponse {
    /// Correlates the originating request.
    pub request_id: u64,
    /// The page's rows, in `(updated_at, id)` order.
    pub records: Vec<WireRecord>,
    /// The keyset point to resume from, present only when `has_more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<SliceCursor>,
    /// Whether further pages remain.
    pub has_more: bool,
}

impl SlicePullResponse {
    /// Builds a non-final page that resumes from `next_cursor`.
    #[must_use]
    pub fn page(request_id: u64, records: Vec<WireRecord>, next_cursor: SliceCursor) -> Self {
        Self {
            request_id,
            records,
            next_cursor: Some(next_cursor),
            has_more: true,
        }
    }

    /// Builds the final page (no more rows follow).
    #[must_use]
    pub fn last(request_id: u64, records: Vec<WireRecord>) -> Self {
        Self {
            request_id,
            records,
            next_cursor: None,
            has_more: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_page_has_no_cursor_and_no_more() {
        let response = SlicePullResponse::last(7, Vec::new());
        assert!(!response.has_more);
        let value = serde_json::to_value(&response).expect("serialize");
        assert!(value.get("next_cursor").is_none());
    }

    #[test]
    fn a_page_carries_its_resume_cursor() {
        let response = SlicePullResponse::page(1, Vec::new(), SliceCursor::new(42, "row-9"));
        assert!(response.has_more);
        let back: SlicePullResponse =
            serde_json::from_str(&serde_json::to_string(&response).expect("ser")).expect("de");
        assert_eq!(back.next_cursor, Some(SliceCursor::new(42, "row-9")));
    }
}
