//! Resolving a sync conflict between a local and a remote row.

use fundamental_core::model::offline::ConflictStrategy;

use crate::engine::{Resolution, SyncMeta};

/// Decides how to reconcile a row that changed both locally and remotely; `Merge` acts as
/// `LastWriteWins` at the row level.
pub fn resolve_conflict(
    strategy: ConflictStrategy,
    local: SyncMeta,
    remote: SyncMeta,
) -> Resolution {
    match strategy {
        ConflictStrategy::ServerWins => Resolution::TakeRemote,
        ConflictStrategy::ClientWins => Resolution::KeepLocal,
        ConflictStrategy::Custom => Resolution::Manual,
        ConflictStrategy::LastWriteWins | ConflictStrategy::Merge => {
            // Ties keep local to avoid churn.
            if remote.updated_at > local.updated_at {
                Resolution::TakeRemote
            } else {
                Resolution::KeepLocal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(updated_at: i64) -> SyncMeta {
        SyncMeta::new(updated_at, 1)
    }

    #[test]
    fn server_wins_always_takes_remote() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::ServerWins, meta(100), meta(1)),
            Resolution::TakeRemote
        );
    }

    #[test]
    fn client_wins_always_keeps_local() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::ClientWins, meta(1), meta(100)),
            Resolution::KeepLocal
        );
    }

    #[test]
    fn last_write_wins_picks_the_newer_side() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::LastWriteWins, meta(1), meta(2)),
            Resolution::TakeRemote
        );
        assert_eq!(
            resolve_conflict(ConflictStrategy::LastWriteWins, meta(2), meta(1)),
            Resolution::KeepLocal
        );
    }

    #[test]
    fn last_write_wins_keeps_local_on_a_tie() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::LastWriteWins, meta(5), meta(5)),
            Resolution::KeepLocal
        );
    }

    #[test]
    fn a_remote_delete_wins_when_it_is_newer() {
        let local = meta(10);
        let remote = SyncMeta::new(20, 2).tombstoned();
        assert_eq!(
            resolve_conflict(ConflictStrategy::LastWriteWins, local, remote),
            Resolution::TakeRemote
        );
    }

    #[test]
    fn custom_defers_to_the_application() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::Custom, meta(1), meta(2)),
            Resolution::Manual
        );
    }

    #[test]
    fn merge_falls_back_to_last_write_wins() {
        assert_eq!(
            resolve_conflict(ConflictStrategy::Merge, meta(1), meta(2)),
            Resolution::TakeRemote
        );
    }
}
