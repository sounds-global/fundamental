use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use fundamental_db::Filter;
use fundamental_sync::wire::{SliceChange, WireRecord, action};
use fundamental_sync::{ChangeError, ChangeStream, SyncChange, record_matches};
use fundamental_ws::{ConnectionId, WsContext, WsEnvelope};
use tracing::{debug, warn};

struct Subscription {
    connection_id: ConnectionId,
    ctx: WsContext,
    table: String,
    /// The base table's key columns, so membership reads key columns from the change's key.
    key_columns: &'static [&'static str],
    filter: Filter,
}

/// Routes each captured [`SyncChange`] to the connections subscribed to it.
///
/// Keyed by owner, so a change never crosses owners.
#[derive(Default)]
pub(crate) struct SliceSubscriptions {
    by_owner: Mutex<HashMap<String, Vec<Subscription>>>,
}

impl SliceSubscriptions {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Registers `ctx`'s subscription to `table` (scoped by `filter`) for `owner`.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub(crate) fn subscribe(
        &self,
        owner: String,
        connection_id: ConnectionId,
        ctx: WsContext,
        table: String,
        key_columns: &'static [&'static str],
        filter: Filter,
    ) {
        let mut by_owner = self.by_owner.lock().unwrap_or_else(PoisonError::into_inner);
        by_owner.entry(owner).or_default().push(Subscription {
            connection_id,
            ctx,
            table,
            key_columns,
            filter,
        });
    }

    /// Pushes `change` to every matching subscription for its owner, pruning disconnected ones.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn fan_out(&self, change: &SyncChange) {
        // The std mutex must not be held across `.await`.
        let targets: Vec<(ConnectionId, WsContext)> = {
            let by_owner = self.by_owner.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(subs) = by_owner.get(&change.owner) else {
                return;
            };
            subs.iter()
                .filter(|s| {
                    s.table == change.table
                        && record_matches(&s.filter, &change.record, s.key_columns)
                })
                .map(|s| (s.connection_id.clone(), s.ctx.clone()))
                .collect()
        };
        if targets.is_empty() {
            return;
        }

        let frame = match serde_json::to_value(SliceChange::new(
            &change.table,
            WireRecord::from(&change.record),
        )) {
            Ok(data) => WsEnvelope::new(action::SLICE_CHANGE, data),
            Err(error) => {
                warn!(error = %error, "failed to encode a slice change for fan-out");
                return;
            }
        };

        let mut dead = Vec::new();
        for (connection_id, ctx) in targets {
            if ctx.send(&frame).await.is_err() {
                dead.push(connection_id);
            }
        }
        if !dead.is_empty() {
            let mut by_owner = self.by_owner.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(subs) = by_owner.get_mut(&change.owner) {
                subs.retain(|s| !dead.contains(&s.connection_id));
            }
            debug!(pruned = dead.len(), "pruned disconnected subscribers");
        }
    }
}

pub(crate) async fn run_fan_out(mut stream: ChangeStream, subscriptions: Arc<SliceSubscriptions>) {
    loop {
        match stream.recv().await {
            Ok(change) => subscriptions.fan_out(&change).await,
            // The stream already warned of the lag.
            Err(ChangeError::Lagged(_)) => {}
            Err(_) => break,
        }
    }
    debug!("slice change fan-out stopped");
}
