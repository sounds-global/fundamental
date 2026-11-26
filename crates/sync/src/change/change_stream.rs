//! A live consumer's stream of captured [`SyncChange`]s.

use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use crate::change::{ChangeError, ChangeResult, SyncChange};

/// A live consumer's end of a [`BroadcastChangePublisher`], yielding each
/// [`SyncChange`] published after it subscribed.
///
/// [`BroadcastChangePublisher`]: crate::change::BroadcastChangePublisher
#[must_use = "a ChangeStream yields nothing unless polled with `recv`"]
pub struct ChangeStream {
    receiver: Receiver<SyncChange>,
}

impl ChangeStream {
    pub(crate) fn new(receiver: Receiver<SyncChange>) -> Self {
        Self { receiver }
    }

    /// Receives the next change, waiting until one is available.
    ///
    /// # Errors
    /// - [`ChangeError::Lagged`] if this consumer fell behind; call `recv` again and re-pull.
    /// - [`ChangeError::Closed`] if the publisher was dropped.
    pub async fn recv(&mut self) -> ChangeResult<SyncChange> {
        match self.receiver.recv().await {
            Ok(change) => Ok(change),
            Err(RecvError::Lagged(skipped)) => {
                warn!(
                    skipped,
                    "change subscriber lagged; dropping to oldest buffered change"
                );
                Err(ChangeError::Lagged(skipped))
            }
            Err(RecvError::Closed) => Err(ChangeError::Closed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::BroadcastChangePublisher;
    use crate::change::ChangePublisher;
    use crate::engine::{SyncMeta, SyncRecord};
    use std::collections::BTreeMap;

    fn change(id: &str) -> SyncChange {
        SyncChange::new(
            "alice",
            "notes",
            SyncRecord::new(id, SyncMeta::new(1, 1), BTreeMap::new()),
            1,
        )
    }

    #[tokio::test]
    async fn a_slow_consumer_lags_then_recovers() {
        let publisher = BroadcastChangePublisher::new(2);
        let mut stream = publisher.subscribe();

        for id in ["a", "b", "c"] {
            publisher.publish(change(id)).await.expect("publish");
        }

        let err = stream.recv().await.expect_err("the consumer lagged");
        assert!(
            matches!(err, ChangeError::Lagged(1)),
            "one change was dropped"
        );

        assert_eq!(stream.recv().await.expect("resumes").record.id, "b");
        assert_eq!(stream.recv().await.expect("resumes").record.id, "c");
    }

    #[tokio::test]
    async fn the_stream_closes_when_the_publisher_is_dropped() {
        let publisher = BroadcastChangePublisher::new(4);
        let mut stream = publisher.subscribe();
        drop(publisher);

        let err = stream.recv().await.expect_err("no publisher remains");
        assert!(matches!(err, ChangeError::Closed));
    }
}
