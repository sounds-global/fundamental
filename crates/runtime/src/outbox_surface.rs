//! [`OutboxSurface`]: a supervised worker that drains a transactional outbox on an interval.

use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, error, info, warn};

use crate::{
    DrainFuture, OutboxDrain, OutboxObserver, StartFuture, Surface, SurfaceGuard, WaitFuture,
};

const DEFAULT_INVENTORY_INTERVAL: Duration = Duration::from_secs(60);

/// Runs an [`OutboxDrain`] on a fixed interval as a supervised [`Surface`], inventorying the outbox beside it.
///
/// A stop never cancels a tick in flight: the rows it claimed would stay leased to a relay that is gone.
#[must_use = "an OutboxSurface does nothing until handed to a Runtime"]
pub struct OutboxSurface {
    name: &'static str,
    drain: Arc<dyn OutboxDrain>,
    poll_interval: Duration,
    inventory_interval: Duration,
    batch: u64,
    observer: Option<Arc<dyn OutboxObserver>>,
}

impl OutboxSurface {
    /// Builds a surface that relays up to `batch` rows through `drain` every `poll_interval`.
    pub fn new(
        name: &'static str,
        drain: Arc<dyn OutboxDrain>,
        poll_interval: Duration,
        batch: u64,
    ) -> Self {
        Self {
            name,
            drain,
            poll_interval,
            inventory_interval: DEFAULT_INVENTORY_INTERVAL,
            batch,
            observer: None,
        }
    }

    /// Reports every tick's outcome to `observer`.
    pub fn with_observer(mut self, observer: Arc<dyn OutboxObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Inventories the outbox every `inventory_interval` rather than every minute.
    pub fn with_inventory_interval(mut self, inventory_interval: Duration) -> Self {
        self.inventory_interval = inventory_interval;
        self
    }
}

impl Surface for OutboxSurface {
    fn name(&self) -> &'static str {
        self.name
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let Self {
                name,
                drain,
                poll_interval,
                inventory_interval,
                batch,
                observer,
            } = *self;
            let final_drain = Arc::clone(&drain);
            let inventory_drain = Arc::clone(&drain);
            let inventory_observer = observer.clone();
            let (stop, mut stopped) = watch::channel(false);
            let handle = tokio::spawn(async move {
                let mut ticker = interval(poll_interval);
                // Skip missed ticks rather than burst to catch up.
                ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    // The stop is heard only here, between ticks: a tick is never cancelled.
                    tokio::select! {
                        biased;
                        _ = stopped.wait_for(|stop| *stop) => break,
                        _ = ticker.tick() => {}
                    }
                    match drain.drain(batch).await {
                        Ok(report) => {
                            if report.relayed > 0 {
                                info!(
                                    surface = name,
                                    relayed = report.relayed,
                                    "outbox batch relayed"
                                );
                            }
                            // Held, not failed: the rows stay in the table where the inventory counts them.
                            if report.set_aside.total() > 0 || report.skipped > 0 {
                                warn!(
                                    surface = name,
                                    set_aside = report.set_aside.total(),
                                    skipped = report.skipped,
                                    "outbox rows were not relayed this tick"
                                );
                            }
                            if let Some(observer) = &observer {
                                observer.drained(name, &report, batch);
                            }
                        }
                        Err(error) => {
                            error!(surface = name, code = error.get().error_code(), %error, "outbox drain tick failed; will retry");
                            if let Some(observer) = &observer {
                                observer.drain_failed(name);
                            }
                        }
                    }
                }
            });
            // Its own task, so a slow or failing inventory never holds up the relay.
            let inventory = tokio::spawn(async move {
                let mut ticker = interval(inventory_interval);
                ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    match inventory_drain.inventory().await {
                        Ok(held) => {
                            if !held.is_clear() {
                                let set_aside: u64 =
                                    held.buckets().map(|(_, count)| count.at_least()).sum();
                                warn!(
                                    surface = name,
                                    set_aside,
                                    beyond_policy = held.beyond_policy().at_least(),
                                    skipped = held.skipped().at_least(),
                                    "outbox holds rows its relay will not send"
                                );
                            }
                            if let Some(observer) = &inventory_observer {
                                observer.inventoried(name, &held);
                            }
                        }
                        Err(error) => {
                            error!(surface = name, code = error.get().error_code(), %error, "outbox inventory failed; will retry");
                            if let Some(observer) = &inventory_observer {
                                observer.inventory_failed(name);
                            }
                        }
                    }
                }
            });
            info!(
                surface = name,
                ?poll_interval,
                ?inventory_interval,
                batch,
                "outbox surface started"
            );
            Ok(Box::new(OutboxGuard {
                name,
                stop,
                handle,
                inventory,
                drain: final_drain,
                batch,
            }) as Box<dyn SurfaceGuard>)
        })
    }
}

struct OutboxGuard {
    name: &'static str,
    stop: watch::Sender<bool>,
    handle: JoinHandle<()>,
    inventory: JoinHandle<()>,
    drain: Arc<dyn OutboxDrain>,
    batch: u64,
}

impl SurfaceGuard for OutboxGuard {
    fn name(&self) -> &'static str {
        self.name
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), fundamental_core::AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            // Stop both loops first so the final tick cannot race a scheduled one; the inventory only reads, so it is aborted.
            self.inventory.abort();
            if self.stop.send(true).is_err() {
                debug!(
                    surface = self.name,
                    "the drain loop had already ended before the stop"
                );
            }
            if let Err(failure) = self.handle.await {
                warn!(surface = self.name, %failure, "the drain loop ended abnormally; draining once more");
            }

            // Then drain once more; a failure is logged, not propagated, so shutdown completes.
            match self.drain.drain(self.batch).await {
                Ok(report) if report.relayed == 0 => {}
                Ok(report) => info!(
                    surface = self.name,
                    relayed = report.relayed,
                    "final outbox batch relayed"
                ),
                Err(error) => warn!(
                    surface = self.name,
                    code = error.get().error_code(),
                    %error,
                    "final outbox drain failed; rows stay claimable for the next boot"
                ),
            }

            debug!(surface = self.name, "outbox surface stopped");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_test::traced_test;

    use fundamental_core::AnyError;
    use fundamental_core::outbox::{DrainReport, HeldCount, OutboxInventory};

    use super::*;

    type ReportFuture<'a> =
        Pin<Box<dyn Future<Output = Result<DrainReport, AnyError>> + Send + 'a>>;
    type InventoryFuture<'a> =
        Pin<Box<dyn Future<Output = Result<OutboxInventory, AnyError>> + Send + 'a>>;

    #[derive(Debug, fundamental_macros::Error)]
    enum Unreachable {
        #[error(code = "TEST_002", message = "the store is gone", status = 503)]
        Store,
    }

    fn clear_inventory() -> InventoryFuture<'static> {
        Box::pin(async {
            Ok(OutboxInventory::new(
                BTreeMap::new(),
                HeldCount::Exact(0),
                HeldCount::Exact(0),
            ))
        })
    }

    struct CountingDrain {
        ticks: Arc<AtomicUsize>,
        relayed_per_tick: usize,
    }

    impl OutboxDrain for CountingDrain {
        fn drain(&self, _batch: u64) -> ReportFuture<'_> {
            let ticks = self.ticks.clone();
            let relayed = self.relayed_per_tick;
            Box::pin(async move {
                ticks.fetch_add(1, Ordering::SeqCst);
                Ok(DrainReport {
                    claimed: relayed,
                    relayed,
                    ..DrainReport::default()
                })
            })
        }

        fn inventory(&self) -> InventoryFuture<'_> {
            clear_inventory()
        }
    }

    struct HeldDrain {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        finished: Arc<AtomicUsize>,
    }

    impl OutboxDrain for HeldDrain {
        fn drain(&self, _batch: u64) -> ReportFuture<'_> {
            Box::pin(async move {
                self.entered.notify_one();
                self.release.notified().await;
                self.finished.fetch_add(1, Ordering::SeqCst);
                Ok(DrainReport::default())
            })
        }

        fn inventory(&self) -> InventoryFuture<'_> {
            clear_inventory()
        }
    }

    #[tokio::test]
    async fn a_stop_lets_the_tick_in_flight_finish_what_it_claimed() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let finished = Arc::new(AtomicUsize::new(0));
        let surface: Box<dyn Surface> = Box::new(OutboxSurface::new(
            "held-outbox",
            Arc::new(HeldDrain {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                finished: Arc::clone(&finished),
            }),
            Duration::from_millis(5),
            16,
        ));
        let guard = surface.start().await.expect("surface starts");
        entered.notified().await;

        let stopping = tokio::spawn(guard.stop());
        tokio::task::yield_now().await;
        assert!(
            !stopping.is_finished(),
            "the stop waits for the tick in flight"
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("the final drain runs once the tick in flight finished");
        release.notify_one();
        stopping.await.expect("joined").expect("stopped");

        assert_eq!(
            finished.load(Ordering::SeqCst),
            2,
            "the tick in flight finished, then the final drain"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn drains_on_a_tick_then_stops_cleanly() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let surface: Box<dyn Surface> = Box::new(OutboxSurface::new(
            "test-outbox",
            Arc::new(CountingDrain {
                ticks: ticks.clone(),
                relayed_per_tick: 3,
            }),
            Duration::from_millis(5),
            16,
        ));
        assert_eq!(surface.name(), "test-outbox");

        let guard = surface.start().await.expect("surface starts");
        assert_eq!(guard.name(), "test-outbox");

        // 42 ms, not a whole multiple of 5 ms, so no tick ties with this sleep's wake-up under virtual time.
        tokio::time::sleep(Duration::from_millis(42)).await;
        let ticked = ticks.load(Ordering::SeqCst);
        assert_eq!(
            ticked, 9,
            "an immediate tick at 0 ms and one every 5 ms through 40 ms is nine drains, and \
             under virtual time that is exact rather than a floor"
        );

        guard.stop().await.expect("stops cleanly");

        let after_stop = ticks.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            ticks.load(Ordering::SeqCst),
            after_stop,
            "a stopped loop must not keep ticking",
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_batch_is_reported_as_such() {
        #[derive(Default)]
        struct Recorder {
            saturated: AtomicUsize,
            partial: AtomicUsize,
            failed: AtomicUsize,
        }

        impl OutboxObserver for Recorder {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drained(&self, _surface: &'static str, report: &DrainReport, batch: u64) {
                if report.saturated(batch) {
                    self.saturated.fetch_add(1, Ordering::SeqCst);
                } else {
                    self.partial.fetch_add(1, Ordering::SeqCst);
                }
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain_failed(&self, _surface: &'static str) {
                self.failed.fetch_add(1, Ordering::SeqCst);
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventoried(&self, _surface: &'static str, _inventory: &OutboxInventory) {}

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory_failed(&self, _surface: &'static str) {}
        }

        let recorder = Arc::new(Recorder::default());
        let surface: Box<dyn Surface> = Box::new(
            OutboxSurface::new(
                "saturated-outbox",
                Arc::new(CountingDrain {
                    ticks: Arc::new(AtomicUsize::new(0)),
                    relayed_per_tick: 4,
                }),
                Duration::from_millis(5),
                4,
            )
            .with_observer(recorder.clone()),
        );

        let guard = surface.start().await.expect("the surface starts");
        tokio::time::sleep(Duration::from_millis(40)).await;
        guard.stop().await.expect("stops cleanly");

        assert!(
            recorder.saturated.load(Ordering::SeqCst) > 0,
            "a relay that returns exactly its batch must be reported as saturated"
        );
        assert_eq!(recorder.partial.load(Ordering::SeqCst), 0);
        assert_eq!(recorder.failed.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    async fn a_tick_that_set_rows_aside_is_a_drain_not_a_failure() {
        use fundamental_core::outbox::{RefusalKind, SetAsideTally};

        struct HoldingDrain;

        impl OutboxDrain for HoldingDrain {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain(&self, _batch: u64) -> ReportFuture<'_> {
                let mut set_aside = SetAsideTally::default();
                set_aside.record(RefusalKind::EnvelopeRefused);
                set_aside.record(RefusalKind::ValueRefused);
                Box::pin(async move {
                    Ok(DrainReport {
                        set_aside,
                        skipped: 1,
                        ..DrainReport::default()
                    })
                })
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory(&self) -> InventoryFuture<'_> {
                clear_inventory()
            }
        }

        #[derive(Default)]
        struct Recorder {
            drained: AtomicUsize,
            set_aside: AtomicUsize,
            skipped: AtomicUsize,
            failed: AtomicUsize,
        }

        impl OutboxObserver for Recorder {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drained(&self, _surface: &'static str, report: &DrainReport, _batch: u64) {
                self.drained.fetch_add(1, Ordering::SeqCst);
                self.set_aside.store(
                    usize::try_from(report.set_aside.total()).unwrap_or(usize::MAX),
                    Ordering::SeqCst,
                );
                self.skipped.store(
                    usize::try_from(report.skipped).unwrap_or(usize::MAX),
                    Ordering::SeqCst,
                );
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain_failed(&self, _surface: &'static str) {
                self.failed.fetch_add(1, Ordering::SeqCst);
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventoried(&self, _surface: &'static str, _inventory: &OutboxInventory) {}

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory_failed(&self, _surface: &'static str) {}
        }

        let recorder = Arc::new(Recorder::default());
        let surface: Box<dyn Surface> = Box::new(
            OutboxSurface::new(
                "holding-outbox",
                Arc::new(HoldingDrain),
                Duration::from_millis(5),
                8,
            )
            .with_observer(recorder.clone()),
        );

        let guard = surface.start().await.expect("the surface starts");
        tokio::time::sleep(Duration::from_millis(40)).await;
        guard.stop().await.expect("stops cleanly");

        assert!(
            recorder.drained.load(Ordering::SeqCst) > 0,
            "a tick that held rows completed and must be reported as a drain"
        );
        assert_eq!(recorder.set_aside.load(Ordering::SeqCst), 2);
        assert_eq!(recorder.skipped.load(Ordering::SeqCst), 1);
        assert_eq!(
            recorder.failed.load(Ordering::SeqCst),
            0,
            "held rows are not a failed claim"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_claim_is_not_an_empty_queue() {
        struct FailingDrain;

        impl OutboxDrain for FailingDrain {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain(&self, _batch: u64) -> ReportFuture<'_> {
                Box::pin(async { Err(AnyError::new(Unreachable::Store)) })
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory(&self) -> InventoryFuture<'_> {
                clear_inventory()
            }
        }

        #[derive(Default)]
        struct Recorder {
            drained: AtomicUsize,
            failed: AtomicUsize,
        }

        impl OutboxObserver for Recorder {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drained(&self, _surface: &'static str, _report: &DrainReport, _batch: u64) {
                self.drained.fetch_add(1, Ordering::SeqCst);
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain_failed(&self, _surface: &'static str) {
                self.failed.fetch_add(1, Ordering::SeqCst);
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventoried(&self, _surface: &'static str, _inventory: &OutboxInventory) {}

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory_failed(&self, _surface: &'static str) {}
        }

        let recorder = Arc::new(Recorder::default());
        let surface: Box<dyn Surface> = Box::new(
            OutboxSurface::new(
                "failing-outbox",
                Arc::new(FailingDrain),
                Duration::from_millis(5),
                8,
            )
            .with_observer(recorder.clone()),
        );

        let guard = surface.start().await.expect("the surface starts");
        tokio::time::sleep(Duration::from_millis(40)).await;
        guard.stop().await.expect("stops cleanly");

        assert!(recorder.failed.load(Ordering::SeqCst) > 0);
        assert_eq!(
            recorder.drained.load(Ordering::SeqCst),
            0,
            "a claim that failed relayed nothing, and must not be counted as a drain"
        );
    }

    #[tokio::test(start_paused = true)]
    #[traced_test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn every_failure_is_logged_with_its_code() {
        struct Broken;

        impl OutboxDrain for Broken {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain(&self, _batch: u64) -> ReportFuture<'_> {
                Box::pin(async { Err(AnyError::new(Unreachable::Store)) })
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory(&self) -> InventoryFuture<'_> {
                Box::pin(async { Err(AnyError::new(Unreachable::Store)) })
            }
        }

        let surface: Box<dyn Surface> = Box::new(OutboxSurface::new(
            "broken-outbox",
            Arc::new(Broken),
            Duration::from_millis(5),
            8,
        ));
        let guard = surface.start().await.expect("the surface starts");
        tokio::time::sleep(Duration::from_millis(40)).await;
        guard.stop().await.expect("stops cleanly");

        logs_assert(|lines: &[&str]| {
            for failure in [
                "outbox drain tick failed",
                "outbox inventory failed",
                "final outbox drain failed",
            ] {
                let coded = lines
                    .iter()
                    .any(|line| line.contains(failure) && line.contains("TEST_002"));
                if !coded {
                    return Err(format!("no line says '{failure}' with its code"));
                }
            }
            Ok(())
        });
    }

    #[derive(Default)]
    struct InventoryRecorder {
        drained: AtomicUsize,
        inventoried: AtomicUsize,
        inventory_failed: AtomicUsize,
    }

    impl OutboxObserver for InventoryRecorder {
        fn drained(&self, _surface: &'static str, _report: &DrainReport, _batch: u64) {
            self.drained.fetch_add(1, Ordering::SeqCst);
        }

        fn drain_failed(&self, _surface: &'static str) {}

        fn inventoried(&self, _surface: &'static str, _inventory: &OutboxInventory) {
            self.inventoried.fetch_add(1, Ordering::SeqCst);
        }

        fn inventory_failed(&self, _surface: &'static str) {
            self.inventory_failed.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_inventory_is_published_at_start_and_on_its_interval() {
        let recorder = Arc::new(InventoryRecorder::default());
        let surface: Box<dyn Surface> = Box::new(
            OutboxSurface::new(
                "inventoried-outbox",
                Arc::new(CountingDrain {
                    ticks: Arc::new(AtomicUsize::new(0)),
                    relayed_per_tick: 0,
                }),
                Duration::from_millis(5),
                8,
            )
            .with_inventory_interval(Duration::from_millis(20))
            .with_observer(recorder.clone()),
        );

        let guard = surface.start().await.expect("the surface starts");
        // 45 ms on a 20 ms interval, so no inventory ties with this sleep's own wake-up.
        tokio::time::sleep(Duration::from_millis(45)).await;
        guard.stop().await.expect("stops cleanly");

        assert_eq!(
            recorder.inventoried.load(Ordering::SeqCst),
            3,
            "one inventory at start and one at 20 ms and 40 ms"
        );
        assert_eq!(recorder.inventory_failed.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_inventory_is_reported_apart_from_the_drain() {
        struct BlindDrain;

        impl OutboxDrain for BlindDrain {
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn drain(&self, _batch: u64) -> ReportFuture<'_> {
                Box::pin(async { Ok(DrainReport::default()) })
            }

            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn inventory(&self) -> InventoryFuture<'_> {
                Box::pin(async { Err(AnyError::new(Unreachable::Store)) })
            }
        }

        let recorder = Arc::new(InventoryRecorder::default());
        let surface: Box<dyn Surface> = Box::new(
            OutboxSurface::new(
                "blind-outbox",
                Arc::new(BlindDrain),
                Duration::from_millis(5),
                8,
            )
            .with_inventory_interval(Duration::from_millis(20))
            .with_observer(recorder.clone()),
        );

        let guard = surface.start().await.expect("the surface starts");
        tokio::time::sleep(Duration::from_millis(45)).await;
        guard.stop().await.expect("stops cleanly");

        assert!(recorder.inventory_failed.load(Ordering::SeqCst) > 0);
        assert_eq!(recorder.inventoried.load(Ordering::SeqCst), 0);
        assert!(
            recorder.drained.load(Ordering::SeqCst) > 0,
            "the relay goes on draining while its inventory fails"
        );
    }
}
