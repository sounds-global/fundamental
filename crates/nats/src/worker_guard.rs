use futures::future::join_all;
use tracing::{info, instrument, warn};

use crate::client::NatsClient;
use crate::error::NatsResult;
use crate::subscription::Listener;

/// A running NATS worker.
///
/// [`drain`](Self::drain) ends it gracefully; dropping it, or [`stop`](Self::stop), aborts every
/// listener and closes the connection.
#[must_use = "dropping the WorkerGuard stops the worker"]
#[derive(Debug)]
pub struct WorkerGuard {
    // `None` only in broker-free tests.
    client: Option<NatsClient>,
    listeners: Vec<Listener>,
}

impl WorkerGuard {
    /// Packages a client and its live listeners into a guard.
    pub(crate) fn new(client: Option<NatsClient>, listeners: Vec<Listener>) -> Self {
        Self { client, listeners }
    }

    /// Drains the worker: every listener finishes what it was handed, then published replies
    /// are flushed and the connection closes.
    ///
    /// The listeners drain together, so one slow handler spends no other's budget.
    ///
    /// # Errors
    /// Returns the first error among the drains and the final flush; every error is logged.
    #[instrument(name = "nats.worker.drain", skip_all, fields(listeners = self.listeners.len()), err(Debug))]
    pub async fn drain(self) -> NatsResult<()> {
        let Self { client, listeners } = self;
        info!("worker draining");
        let mut first_error = None;
        for outcome in join_all(listeners.into_iter().map(Listener::drain)).await {
            if let Err(error) = outcome {
                warn!(%error, "a listener did not drain cleanly");
                first_error.get_or_insert(error);
            }
        }
        // Flush first, or the drain's replies are stranded in the write buffer.
        if let Some(client) = client.as_ref()
            && let Err(error) = client.flush().await
        {
            warn!(%error, "the worker's final flush failed; replies published during the drain may be lost");
            first_error.get_or_insert(error);
        }
        drop(client);
        info!("worker drained");
        first_error.map_or(Ok(()), Err)
    }

    /// Stops the worker at once, mid-message if need be; prefer [`drain`](Self::drain).
    pub fn stop(self) {
        let Self { client, listeners } = self;
        info!(listeners = listeners.len(), "worker stopping");
        drop(listeners);
        drop(client);
    }

    /// The number of listeners kept alive.
    #[cfg(test)]
    pub(crate) fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    const WORK: Duration = Duration::from_secs(10);

    /// Listeners drain side by side, not in turn.
    #[tokio::test(start_paused = true)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn draining_finishes_every_listener_together() {
        let finished = Arc::new(AtomicUsize::new(0));
        let listeners = (0..2).map(|_| {
            let finished = Arc::clone(&finished);
            Listener::spawn(|mut requested| async move {
                requested.arrived().await;
                tokio::time::sleep(WORK).await;
                finished.fetch_add(1, Ordering::SeqCst);
            })
        });
        let guard = WorkerGuard::new(None, listeners.collect());
        let started = tokio::time::Instant::now();
        guard.drain().await.expect("the worker drains");
        assert_eq!(
            finished.load(Ordering::SeqCst),
            2,
            "a listener's work was cut short"
        );
        assert!(
            started.elapsed() < WORK * 2,
            "the listeners drained one after another: {:?}",
            started.elapsed()
        );
    }
}
