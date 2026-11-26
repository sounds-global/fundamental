//! [`ChangeFrame`] — the NATS wire form of a captured [`SyncChange`].

use fundamental_macros::Message;
use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::change::SyncChange;
use crate::engine::SyncRecord;
use crate::wire::WireRecord;

/// The NATS form of a [`SyncChange`], published to `sync.change.{owner}.{table}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Message)]
#[subject_fn = "nats_subject"]
pub struct ChangeFrame {
    /// The owning principal.
    pub owner: String,
    /// The base table the change occurred in.
    pub table: String,
    /// The changed row (a tombstone for a delete).
    pub record: WireRecord,
    /// The change's commit timestamp in milliseconds since the Unix epoch.
    pub commit_ts: i64,
}

impl ChangeFrame {
    fn nats_subject(&self) -> String {
        format!("sync.change.{}.{}", self.owner, self.table)
    }
}

impl From<&SyncChange> for ChangeFrame {
    fn from(change: &SyncChange) -> Self {
        Self {
            owner: change.owner.clone(),
            table: change.table.clone(),
            record: WireRecord::from(&change.record),
            commit_ts: change.commit_ts,
        }
    }
}

impl TryFrom<ChangeFrame> for SyncChange {
    type Error = SyncError;

    ///
    /// # Errors
    /// Returns [`SyncError::Protocol`] if the wire record carries invalid data.
    fn try_from(frame: ChangeFrame) -> Result<Self, Self::Error> {
        let record = SyncRecord::try_from(frame.record)?;
        Ok(SyncChange::new(
            frame.owner,
            frame.table,
            record,
            frame.commit_ts,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SyncMeta;
    use std::collections::BTreeMap;

    fn change(id: &str) -> SyncChange {
        SyncChange::new(
            "alice",
            "notes",
            SyncRecord::new(id, SyncMeta::new(10, 1), BTreeMap::new()),
            10,
        )
    }

    #[test]
    fn round_trips_through_the_frame() {
        let original = change("n1");
        let frame = ChangeFrame::from(&original);
        assert_eq!(frame.owner, "alice");
        assert_eq!(frame.table, "notes");
        let back = SyncChange::try_from(frame).expect("valid frame");
        assert_eq!(back, original);
    }

    #[test]
    fn subject_is_keyed_by_owner_and_table() {
        assert_eq!(
            ChangeFrame::from(&change("n1")).nats_subject(),
            "sync.change.alice.notes"
        );
    }
}
