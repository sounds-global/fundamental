//! An in-process [`ChangePublisher`] over a Tokio broadcast channel.

use tokio::sync::broadcast::{Sender, channel};
use tracing::{debug, instrument};

use crate::change::{ChangePublisher, ChangeResult, ChangeStream, SyncChange};

/// A [`ChangePublisher`] that fans changes out in-process to every live [`ChangeStream`]; a
/// lagging subscriber sees `Lagged` rather than blocking it.
#[derive(Clone)]
pub struct BroadcastChangePublisher {
    sender: Sender<SyncChange>,
}

impl BroadcastChangePublisher {
    /// Creates a publisher buffering up to `capacity` unread changes per subscriber.
    ///
    /// # Panics
    /// Panics if `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = channel(capacity);
        Self { sender }
    }

    /// Subscribes a new live consumer; it receives every change published after this call.
    pub fn subscribe(&self) -> ChangeStream {
        ChangeStream::new(self.sender.subscribe())
    }

    /// The number of live subscribers currently attached.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl ChangePublisher for BroadcastChangePublisher {
    #[instrument(
        skip_all,
        fields(table = %change.table),
        err(Debug)
    )]
    async fn publish(&self, change: SyncChange) -> ChangeResult<()> {
        // No live subscribers is not a failure: this fan-out is best-effort live delivery.
        match self.sender.send(change) {
            Ok(delivered) => {
                debug!(delivered, "change fanned out to live subscribers");
                Ok(())
            }
            Err(_) => {
                debug!("change published with no live subscribers");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{SyncMeta, SyncRecord};
    use std::collections::BTreeMap;

    fn change(owner: &str, id: &str) -> SyncChange {
        SyncChange::new(
            owner,
            "notes",
            SyncRecord::new(id, SyncMeta::new(1, 1), BTreeMap::new()),
            1,
        )
    }

    #[tokio::test]
    async fn a_published_change_reaches_a_subscriber() {
        let publisher = BroadcastChangePublisher::new(8);
        let mut stream = publisher.subscribe();

        publisher
            .publish(change("alice", "n1"))
            .await
            .expect("publish");

        let received = stream.recv().await.expect("receive");
        assert_eq!(received.owner, "alice");
        assert_eq!(received.record.id, "n1");
    }

    #[tokio::test]
    async fn every_live_subscriber_receives_the_change() {
        let publisher = BroadcastChangePublisher::new(8);
        let mut a = publisher.subscribe();
        let mut b = publisher.subscribe();
        assert_eq!(publisher.subscriber_count(), 2);

        publisher
            .publish(change("alice", "n1"))
            .await
            .expect("publish");

        assert_eq!(a.recv().await.expect("a").record.id, "n1");
        assert_eq!(b.recv().await.expect("b").record.id, "n1");
    }

    #[tokio::test]
    async fn publishing_with_no_subscribers_is_not_an_error() {
        let publisher = BroadcastChangePublisher::new(8);
        publisher
            .publish(change("alice", "n1"))
            .await
            .expect("publishing into the void still succeeds");
        assert_eq!(publisher.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn a_subscriber_only_sees_changes_published_after_it_subscribed() {
        let publisher = BroadcastChangePublisher::new(8);
        publisher
            .publish(change("alice", "before"))
            .await
            .expect("publish");

        let mut late = publisher.subscribe();
        publisher
            .publish(change("alice", "after"))
            .await
            .expect("publish");

        assert_eq!(late.recv().await.expect("recv").record.id, "after");
    }
}
