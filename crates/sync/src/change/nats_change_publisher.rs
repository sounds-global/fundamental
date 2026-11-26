//! A NATS-backed [`ChangePublisher`] and the bridge that delivers its changes
//! into a node's in-process fan-out.

use fundamental_nats::{
    Listener, NatsClient, ReceivedMessage, SubjectPattern, SubscriptionOptions,
};
use tracing::{instrument, warn};

use crate::change::{
    BroadcastChangePublisher, ChangeError, ChangeFrame, ChangePublisher, ChangeResult, SyncChange,
};

const ALL_CHANGES: &str = "sync.change.>";

/// A [`ChangePublisher`] that fans changes out across nodes over NATS, one subject per owner and
/// table.
#[derive(Clone)]
pub struct NatsChangePublisher {
    client: NatsClient,
}

impl NatsChangePublisher {
    /// Wraps a connected NATS client.
    #[must_use]
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

impl ChangePublisher for NatsChangePublisher {
    #[instrument(
        skip_all,
        fields(table = %change.table),
        err(Debug)
    )]
    async fn publish(&self, change: SyncChange) -> ChangeResult<()> {
        self.client
            .publish(ChangeFrame::from(&change))
            .await
            .map_err(|e| ChangeError::Publish(e.to_string()))?;
        Ok(())
    }
}

/// Republishes every change on the bus into `local`; runs until the returned [`Listener`] drops.
///
/// # Errors
/// Returns [`ChangeError::Publish`] if the subscription cannot be established.
pub async fn nats_change_bridge(
    client: &NatsClient,
    local: BroadcastChangePublisher,
) -> ChangeResult<Listener> {
    let pattern =
        SubjectPattern::new(ALL_CHANGES).map_err(|e| ChangeError::Publish(e.to_string()))?;
    client
        .subscribe_dispatch::<ChangeFrame, _, _>(
            pattern,
            SubscriptionOptions::default(),
            move |message: ReceivedMessage<ChangeFrame>| {
                let local = local.clone();
                async move {
                    match SyncChange::try_from(message.payload) {
                        Ok(change) => {
                            let _ = local.publish(change).await;
                        }
                        Err(error) => {
                            warn!(error = %error, "dropping a malformed change received from NATS");
                        }
                    }
                    Ok(())
                }
            },
        )
        .await
        .map_err(|e| ChangeError::Publish(e.to_string()))
}
