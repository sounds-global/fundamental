//! The bidirectional sync driver.

use tracing::field::Empty;
use tracing::{Span, debug, info, instrument, warn};

use crate::SyncError;
use crate::engine::{
    LocalMirror, PullOutcome, RemoteSource, Resolution, SliceBinding, resolve_conflict,
};

/// Drives sync between a [`LocalMirror`] and a [`RemoteSource`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncEngine;

impl SyncEngine {
    /// Pushes the dirty rows to the remote, marking each clean once accepted, and returns how many.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if reading the dirty set, the remote write, or the
    /// clean-marking fails.
    #[instrument(
        skip(self, local, remote, binding),
        fields(slice = %binding.slice(), table = %binding.local_table(), pushed = Empty),
        err(Debug)
    )]
    pub async fn push<L, R>(
        &self,
        local: &L,
        remote: &R,
        binding: &SliceBinding,
    ) -> Result<usize, SyncError>
    where
        L: LocalMirror,
        R: RemoteSource,
    {
        let table = binding.local_table();
        let dirty = local.dirty(table).await?;
        for record in &dirty {
            debug!(id = %record.id, tombstone = record.meta.tombstone, "pushing local change");
            remote
                .upsert(binding.slice(), binding.params(), record)
                .await?;
            local.mark_clean(table, &record.id).await?;
        }
        Span::current().record("pushed", dirty.len());
        info!(pushed = dirty.len(), "push complete");
        Ok(dirty.len())
    }

    /// Pulls rows changed on the remote since the last pull, resolving conflicts with `strategy`, and
    /// advances the high-water mark.
    ///
    /// # Errors
    /// Returns a [`SyncError`] if reading the remote, reading or writing the
    /// mirror, or updating the high-water mark fails.
    #[instrument(
        skip(self, local, remote, binding),
        fields(slice = %binding.slice(), table = %binding.local_table(), strategy = binding.strategy().as_str(), applied = Empty, conflicts = Empty),
        err(Debug)
    )]
    pub async fn pull<L, R>(
        &self,
        local: &L,
        remote: &R,
        binding: &SliceBinding,
    ) -> Result<PullOutcome, SyncError>
    where
        L: LocalMirror,
        R: RemoteSource,
    {
        let table = binding.local_table();
        let strategy = binding.strategy();
        let since = local.last_pulled(table).await?;
        let changed = remote
            .changed_since(binding.slice(), binding.params(), since)
            .await?;
        debug!(since, candidates = changed.len(), "pulled remote changes");

        let mut outcome = PullOutcome::default();
        let mut high_water = since;

        for record in &changed {
            high_water = high_water.max(record.meta.updated_at);

            match local.get(table, &record.id).await? {
                None => {
                    local.apply_remote(table, record).await?;
                    outcome.applied += 1;
                }
                Some(local_row) => match resolve_conflict(strategy, local_row.meta, record.meta) {
                    Resolution::TakeRemote => {
                        debug!(id = %record.id, "remote wins; applying");
                        local.apply_remote(table, record).await?;
                        outcome.applied += 1;
                    }
                    Resolution::KeepLocal => {
                        debug!(id = %record.id, "local wins; keeping");
                        outcome.kept_local += 1;
                    }
                    Resolution::Manual => {
                        // The strategy cannot decide; surface it rather than lose data.
                        warn!(
                            id = %record.id,
                            "unresolved conflict left for manual resolution"
                        );
                        outcome.conflicts += 1;
                    }
                },
            }
        }

        local.set_last_pulled(table, high_water).await?;
        let span = Span::current();
        span.record("applied", outcome.applied);
        span.record("conflicts", outcome.conflicts);
        info!(
            applied = outcome.applied,
            kept_local = outcome.kept_local,
            conflicts = outcome.conflicts,
            "pull complete"
        );
        Ok(outcome)
    }
}
