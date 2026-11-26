//! [`SpawnedSurface`]: an arbitrary long-running task, supervised.

use std::future::Future;

use tokio::task::JoinHandle;
use tracing::info;

use crate::{DrainFuture, StartFuture, Surface, SurfaceGuard, WaitFuture};

/// Runs a caller-supplied future as a supervised [`Surface`], aborting it on drain.
///
/// ```rust,no_run
/// # extern crate fundamental_runtime;
/// # extern crate tokio;
/// # use fundamental_runtime::{Runtime, SpawnedSurface};
/// # async fn example(mut events: tokio::sync::mpsc::UnboundedReceiver<u8>) {
/// let feed = SpawnedSurface::new("status-feed", async move {
///     let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
///     loop {
///         tokio::select! {
///             _ = ticker.tick() => { /* full reconcile */ }
///             Some(_event) = events.recv() => { /* apply and republish now */ }
///         }
///     }
/// });
///
/// Runtime::new().with(feed).run().await.expect("clean shutdown");
/// # }
/// ```
///
/// # The task must be abort-safe
///
/// Drain aborts the task at its next await point, so the work must be safe to interrupt anywhere.
#[must_use = "a SpawnedSurface does nothing until handed to a Runtime"]
pub struct SpawnedSurface<F> {
    name: &'static str,
    task: F,
}

impl<F> SpawnedSurface<F>
where
    F: Future<Output = ()> + Send + 'static,
{
    /// Builds a surface that runs `task`, reporting itself as `name`.
    pub const fn new(name: &'static str, task: F) -> Self {
        Self { name, task }
    }
}

impl<F> Surface for SpawnedSurface<F>
where
    F: Future<Output = ()> + Send + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let Self { name, task } = *self;
            let handle = tokio::spawn(task);
            info!(surface = name, "spawned surface started");
            Ok(Box::new(SpawnedGuard { name, handle }) as Box<dyn SurfaceGuard>)
        })
    }
}

struct SpawnedGuard {
    name: &'static str,
    handle: JoinHandle<()>,
}

impl SurfaceGuard for SpawnedGuard {
    fn name(&self) -> &'static str {
        self.name
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        // A task that returns has stopped doing its job; the runtime brings the process down.
        Box::pin(async move {
            let _ = (&mut self.handle).await;
            Ok(())
        })
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            self.handle.abort();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[tokio::test(start_paused = true)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn the_task_runs_and_is_aborted_on_drain() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let counter = ticks.clone();

        let surface: Box<dyn Surface> = Box::new(SpawnedSurface::new("counting", async move {
            loop {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }));
        assert_eq!(surface.name(), "counting");

        let guard = surface.start().await.expect("starts");
        assert_eq!(guard.name(), "counting");
        // 42 ms, not a whole multiple of 5 ms, so no tick ties with this sleep's wake-up under virtual time.
        tokio::time::sleep(Duration::from_millis(42)).await;
        assert_eq!(
            ticks.load(Ordering::SeqCst),
            9,
            "the body counts before it sleeps, so a 5 ms turn lands ticks at 0 ms through \
             40 ms — nine of them, exact under virtual time, where the old floor of two was \
             all a real clock could promise"
        );

        guard.stop().await.expect("stops cleanly");

        let after_stop = ticks.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(ticks.load(Ordering::SeqCst), after_stop);
    }

    #[tokio::test]
    async fn a_task_that_ends_on_its_own_resolves_wait() {
        let surface: Box<dyn Surface> = Box::new(SpawnedSurface::new("brief", async {}));
        let mut guard = surface.start().await.expect("starts");

        tokio::time::timeout(Duration::from_secs(1), guard.wait())
            .await
            .expect("wait resolves once the task ends")
            .expect("a clean end");
    }
}
