use std::sync::Arc;

use fundamental_nats::{
    Listener, NatsClient, ReceivedMessage, SubjectPattern, SubscriptionOptions,
};
use fundamental_sync::change::{ChangeError, ChangeFrame, ChangeResult, SyncChange};
use fundamental_sync::{SliceReader, SliceWriteStore};
use tracing::warn;

use crate::projection::{ProjectionRegistry, ProjectionWriter};

const ALL_CHANGES: &str = "sync.change.>";

/// The queue group that hands each change to exactly one consumer in the cluster.
const PROJECTION_QUEUE_GROUP: &str = "fundamental.projections";

/// Subscribes to every captured change on the NATS bus under a shared queue group and
/// recomputes the projections each change affects, writing them into `store`.
///
/// Run it on every node: the queue group keeps a single writer per change. The consumer runs
/// until the returned [`Listener`] drops; a malformed change is logged and skipped.
///
/// # Errors
/// Returns [`ChangeError::Publish`] if the subscription cannot be established.
pub async fn nats_projection_consumer<S>(
    client: &NatsClient,
    registry: Arc<ProjectionRegistry>,
    store: Arc<S>,
) -> ChangeResult<Listener>
where
    S: SliceWriteStore + Send + Sync + 'static,
{
    let pattern =
        SubjectPattern::new(ALL_CHANGES).map_err(|e| ChangeError::Publish(e.to_string()))?;
    let options = SubscriptionOptions::default().with_queue_group(PROJECTION_QUEUE_GROUP);
    let writer = Arc::new(ProjectionWriter::new(store.clone()));
    let reader: Arc<dyn SliceReader> = store;

    client
        .subscribe_dispatch::<ChangeFrame, _, _>(
            pattern,
            options,
            move |message: ReceivedMessage<ChangeFrame>| {
                let registry = registry.clone();
                let writer = writer.clone();
                let reader = reader.clone();
                async move {
                    match SyncChange::try_from(message.payload) {
                        Ok(change) => registry.apply(&change, &reader, writer.as_ref()).await,
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
