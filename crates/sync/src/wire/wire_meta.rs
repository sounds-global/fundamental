//! [`WireMeta`] — the wire form of a row's sync-overlay metadata.

use serde::{Deserialize, Serialize};

use crate::SyncMeta;

/// The on-the-wire form of [`SyncMeta`]: the conflict-resolution overlay of a synced row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMeta {
    /// Server-authoritative last-modified time, in milliseconds since the epoch.
    pub updated_at: i64,
    /// Monotonic version counter.
    pub version: i64,
    /// Whether the row is a tombstone (a propagated delete).
    pub tombstone: bool,
}

impl From<SyncMeta> for WireMeta {
    fn from(meta: SyncMeta) -> Self {
        Self {
            updated_at: meta.updated_at,
            version: meta.version,
            tombstone: meta.tombstone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_sync_meta_preserves_fields_and_tombstone() {
        let meta = SyncMeta::new(42, 7).tombstoned();
        let wire = WireMeta::from(meta);
        assert_eq!(wire.updated_at, 42);
        assert_eq!(wire.version, 7);
        assert!(wire.tombstone);
    }

    #[test]
    fn serializes_with_stable_keys() {
        let wire = WireMeta::from(SyncMeta::new(1, 2));
        let value = serde_json::to_value(wire).expect("serialize");
        assert_eq!(value["updated_at"], 1);
        assert_eq!(value["version"], 2);
        assert_eq!(value["tombstone"], false);
    }
}
