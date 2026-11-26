//! [`PeriodicSurface`]: a supervised worker that runs a [`PeriodicTask`] on an interval.

use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, error, info};

use crate::{DrainFuture, PeriodicTask, StartFuture, Surface, SurfaceGuard, WaitFuture};

/// Runs a [`PeriodicTask`] on a fixed interval as a supervised [`Surface`], first at start unless
/// [`after_first_interval`](Self::after_first_interval) says otherwise.
#[must_use = "a PeriodicSurface does nothing until handed to a Runtime"]
pub struct PeriodicSurface {
    name: &'static str,
    task: Arc<dyn PeriodicTask>,
    interval: Duration,
    run_immediately: bool,
}

impl PeriodicSurface {
    /// Builds a surface that runs `task` immediately and then every `interval`, reporting itself as `name`.
    pub fn new(name: &'static str, task: Arc<dyn PeriodicTask>, interval: Duration) -> Self {
        Self {
            name,
            task,
            interval,
            run_immediately: true,
        }
    }

    /// Waits one full `interval` before the first run, so a fleet restart is not a stampede.
    pub fn after_first_interval(mut self) -> Self {
        self.run_immediately = false;
        self
    }
}

impl Surface for PeriodicSurface {
    fn name(&self) -> &'static str {
        self.name
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let Self {
                name,
                task,
                interval: period,
                run_immediately,
            } = *self;
            let handle = tokio::spawn(async move {
                let mut ticker = interval(period);
                // Skip missed ticks rather than burst to catch up.
                ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                if !run_immediately {
                    // `interval` yields its first tick at once; consume it to wait a full period.
                    ticker.tick().await;
                }
                loop {
                    ticker.tick().await;
                    if let Err(error) = task.run().await {
                        error!(surface = name, %error, "periodic task tick failed; will retry");
                    }
                }
            });
            info!(surface = name, ?period, "periodic surface started");
            Ok(Box::new(PeriodicGuard { name, handle }) as Box<dyn SurfaceGuard>)
        })
    }
}

struct PeriodicGuard {
    name: &'static str,
    handle: JoinHandle<()>,
}

impl SurfaceGuard for PeriodicGuard {
    fn name(&self) -> &'static str {
        self.name
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), fundamental_core::AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            self.handle.abort();
            debug!(surface = self.name, "periodic surface stopped");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct CountingTask {
        runs: Arc<AtomicUsize>,
    }

    impl PeriodicTask for CountingTask {
        fn run(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), fundamental_core::AnyError>>
                    + Send
                    + '_,
            >,
        > {
            let runs = self.runs.clone();
            Box::pin(async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_run_is_immediate_by_default() {
        let runs = Arc::new(AtomicUsize::new(0));
        let surface: Box<dyn Surface> = Box::new(PeriodicSurface::new(
            "immediate",
            Arc::new(CountingTask { runs: runs.clone() }),
            Duration::from_secs(3600),
        ));

        let guard = surface.start().await.expect("surface starts");
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "the first tick fires at once, well inside the hour-long period"
        );
        guard.stop().await.expect("stops cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn after_first_interval_waits_a_full_period() {
        let runs = Arc::new(AtomicUsize::new(0));
        let surface: Box<dyn Surface> = Box::new(
            PeriodicSurface::new(
                "delayed",
                Arc::new(CountingTask { runs: runs.clone() }),
                Duration::from_secs(3600),
            )
            .after_first_interval(),
        );

        let guard = surface.start().await.expect("surface starts");
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(
            runs.load(Ordering::SeqCst),
            0,
            "nothing may run until a full interval has elapsed"
        );
        guard.stop().await.expect("stops cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn runs_on_a_tick_then_stops_cleanly() {
        let runs = Arc::new(AtomicUsize::new(0));
        let surface: Box<dyn Surface> = Box::new(PeriodicSurface::new(
            "test-periodic",
            Arc::new(CountingTask { runs: runs.clone() }),
            Duration::from_millis(5),
        ));
        assert_eq!(surface.name(), "test-periodic");

        let guard = surface.start().await.expect("surface starts");
        assert_eq!(guard.name(), "test-periodic");

        // 42 ms, not a whole multiple of 5 ms, so no tick ties with this sleep's wake-up under virtual time.
        tokio::time::sleep(Duration::from_millis(42)).await;
        let ran = runs.load(Ordering::SeqCst);
        assert_eq!(
            ran, 9,
            "an immediate tick at 0 ms and one every 5 ms through 40 ms is nine runs, and \
             under virtual time that is exact rather than approximate"
        );

        guard.stop().await.expect("stops cleanly");

        let after_stop = runs.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            runs.load(Ordering::SeqCst),
            after_stop,
            "aborted loop must not keep running",
        );
    }
}
