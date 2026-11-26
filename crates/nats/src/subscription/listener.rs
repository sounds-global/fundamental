use std::future::Future;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{info, instrument};

use super::declared_subscription::DeclaredSubscription;
use super::drain_requested::DrainRequested;
use super::subscription_inventory::SubscriptionInventory;
use crate::error::{NatsError, NatsResult};

/// A running message-dispatch loop.
///
/// [`drain`](Self::drain) on shutdown, so a queue-group member answers what it was handed;
/// [`stop`](Self::stop) or dropping aborts now; [`join`](Self::join) awaits the natural end.
#[must_use = "dropping the Listener immediately stops dispatching messages"]
#[derive(Debug)]
pub struct Listener {
    handle: Option<JoinHandle<()>>,
    /// Asks the loop to drain. `None` once asked, or for a loop spawned in a test.
    drain: Option<oneshot::Sender<()>>,
    /// What to withdraw from the readiness inventory when this guard goes away.
    ///
    /// Withdrawn with the loop, so readiness never answers for a subscription nobody holds.
    tracked: Option<(Arc<SubscriptionInventory>, Vec<DeclaredSubscription>)>,
}

impl Listener {
    /// Spawns the loop `dispatch_loop` builds as the background task and returns its guard.
    ///
    /// The loop must select on its [`DrainRequested`] beside the next delivery, or
    /// [`drain`](Self::drain) waits for a close that never comes.
    pub fn spawn<L, F>(dispatch_loop: L) -> Self
    where
        L: FnOnce(DrainRequested) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        let (drain, requested) = DrainRequested::pair();
        Self {
            handle: Some(tokio::spawn(dispatch_loop(requested))),
            drain: Some(drain),
            tracked: None,
        }
    }

    /// Spawns the loop and ties `declared` to this guard's lifetime, so readiness
    /// answers for these subscriptions exactly as long as the loop runs.
    pub(crate) fn spawn_tracked<L, F>(
        inventory: Arc<SubscriptionInventory>,
        declared: Vec<DeclaredSubscription>,
        dispatch_loop: L,
    ) -> Self
    where
        L: FnOnce(DrainRequested) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        let mut listener = Self::spawn(dispatch_loop);
        listener.tracked = Some((inventory, declared));
        listener
    }

    /// Unsubscribes, finishes every delivery already routed to this loop, and resolves
    /// once the loop has ended.
    ///
    ///
    /// Dropping the future aborts the loop, so a timeout bounds a drain.
    ///
    /// # Errors
    /// Returns [`NatsError::Internal`] if the dispatch task panicked.
    #[instrument(name = "nats.listener.drain", skip_all, err(Debug))]
    pub async fn drain(mut self) -> NatsResult<()> {
        if let Some(drain) = self.drain.take() {
            // A refused send means the loop already ended.
            if drain.send(()).is_err() {
                info!("the dispatch loop had already ended before its drain was asked for");
            }
        }
        self.await_loop().await?;
        info!("dispatch loop drained");
        Ok(())
    }

    /// Stops the dispatch loop, aborting its background task.
    pub fn stop(mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    /// Awaits the dispatch loop's natural completion, when the subscription closes.
    ///
    /// # Errors
    /// Returns [`NatsError::Internal`] if the dispatch task panicked.
    pub async fn join(mut self) -> NatsResult<()> {
        self.await_loop().await
    }

    async fn await_loop(&mut self) -> NatsResult<()> {
        if let Some(handle) = self.handle.as_mut() {
            let outcome = handle.await;
            self.handle = None;
            outcome.map_err(|e| NatsError::internal(format!("dispatch task panicked: {e}")))?;
        }
        Ok(())
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
        if let Some((inventory, declared)) = &self.tracked {
            for entry in declared {
                inventory.withdraw(entry);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use fundamental_core::Error as _;

    #[tokio::test]
    async fn join_resolves_when_the_loop_finishes() {
        let listener = Listener::spawn(|_| async {});
        listener.join().await.expect("join resolves Ok");
    }

    /// How long the loops below wait before they would record having run.
    ///
    /// Advanced past under a virtual clock, so an unset flag proves an abort.
    const LOOP_DELAY: Duration = Duration::from_secs(10);

    /// A panic inside the loop surfaces through `join` as an internal error.
    #[tokio::test]
    async fn join_reports_a_panicked_loop() {
        let listener = Listener::spawn(|_| async { panic!("loop blew up") });
        let err = listener.join().await.expect_err("panic surfaces");
        assert_eq!(err.error_code(), "NATS_016");
    }

    /// Dropping the guard aborts the loop, proved under a virtual clock.
    #[tokio::test(start_paused = true)]
    async fn drop_aborts_the_loop() {
        let reached = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&reached);
        let listener = Listener::spawn(|_| async move {
            tokio::time::sleep(LOOP_DELAY).await;
            flag.store(true, Ordering::SeqCst);
        });
        drop(listener);
        tokio::time::sleep(LOOP_DELAY * 2).await;
        assert!(
            !reached.load(Ordering::SeqCst),
            "the aborted loop must not reach its post-sleep work, and the clock is now well \
             past the point at which a live one would have"
        );
    }

    /// A tracked listener withdraws its declarations when it goes away, `stop` included.
    #[tokio::test]
    async fn stopping_a_tracked_listener_withdraws_its_subscriptions() {
        let inventory = Arc::new(SubscriptionInventory::default());
        let declared = DeclaredSubscription {
            subject: "a.b".to_string(),
            queue_group: None,
        };
        let listener =
            Listener::spawn_tracked(Arc::clone(&inventory), vec![declared.clone()], |_| async {
                std::future::pending::<()>().await
            });
        inventory.declare(declared.clone());
        inventory.refuse(
            &crate::subscription::permissions_violation::PermissionsViolation {
                subject: "a.b".to_string(),
                queue_group: None,
            },
        );
        assert!(inventory.refused().is_some());

        listener.stop();
        assert_eq!(
            inventory.refused(),
            None,
            "a stopped listener must not keep the service unready"
        );
    }

    /// `drain` resolves only after the loop finishes its remaining work.
    #[tokio::test(start_paused = true)]
    async fn drain_resolves_after_the_loop_finishes_its_work() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        let listener = Listener::spawn(|mut requested| async move {
            requested.arrived().await;
            tokio::time::sleep(LOOP_DELAY).await;
            flag.store(true, Ordering::SeqCst);
        });
        listener.drain().await.expect("the loop drains");
        assert!(
            finished.load(Ordering::SeqCst),
            "drain resolved before the loop finished the work it held"
        );
    }

    /// Dropping the `drain` future aborts the loop.
    #[tokio::test(start_paused = true)]
    async fn a_drain_abandoned_by_its_budget_aborts_the_loop() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        let listener = Listener::spawn(|mut requested| async move {
            requested.arrived().await;
            tokio::time::sleep(LOOP_DELAY).await;
            flag.store(true, Ordering::SeqCst);
        });
        let budget = tokio::time::timeout(LOOP_DELAY / 2, listener.drain()).await;
        assert!(budget.is_err(), "the drain outlasts its budget");
        tokio::time::sleep(LOOP_DELAY * 2).await;
        assert!(
            !finished.load(Ordering::SeqCst),
            "a drain abandoned by its budget must abort the loop, and the clock is past the \
             point a surviving loop would have finished"
        );
    }

    #[tokio::test]
    async fn draining_an_ended_loop_resolves() {
        let listener = Listener::spawn(|_| async {});
        tokio::task::yield_now().await;
        listener.drain().await.expect("an ended loop drains");
    }

    /// `stop` aborts the loop as dropping does.
    #[tokio::test(start_paused = true)]
    async fn stop_aborts_the_loop() {
        let reached = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&reached);
        let listener = Listener::spawn(|_| async move {
            tokio::time::sleep(LOOP_DELAY).await;
            flag.store(true, Ordering::SeqCst);
        });
        listener.stop();
        tokio::time::sleep(LOOP_DELAY * 2).await;
        assert!(
            !reached.load(Ordering::SeqCst),
            "a stopped loop must not reach its post-sleep work, past its own deadline"
        );
    }
}
