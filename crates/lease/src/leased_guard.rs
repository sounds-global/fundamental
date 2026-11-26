use fundamental_core::AnyError;
use fundamental_runtime::{DrainFuture, SurfaceGuard, WaitFuture};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::{Lease, LeaseError};

/// The running half of a [`LeasedSurface`](crate::LeasedSurface).
pub(crate) struct LeasedGuard {
    lease: Lease,
    stop_work: watch::Sender<bool>,
    stop_renewal: watch::Sender<bool>,
    ended: watch::Receiver<bool>,
    workers: Vec<JoinHandle<()>>,
    renewal: JoinHandle<()>,
}

impl LeasedGuard {
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub(crate) fn new(
        lease: Lease,
        stop_work: watch::Sender<bool>,
        stop_renewal: watch::Sender<bool>,
        ended: watch::Receiver<bool>,
        workers: Vec<JoinHandle<()>>,
        renewal: JoinHandle<()>,
    ) -> Self {
        Self {
            lease,
            stop_work,
            stop_renewal,
            ended,
            workers,
            renewal,
        }
    }
}

impl SurfaceGuard for LeasedGuard {
    fn name(&self) -> &'static str {
        self.lease.name().as_str()
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(async move {
            // A surface that can no longer hold its lease must bring the process down.
            while !*self.ended.borrow_and_update() {
                if self.ended.changed().await.is_err() {
                    break;
                }
            }
            Err(AnyError::new(LeaseError::SurfaceEnded {
                lease: self.lease.name().as_str().to_owned(),
                detail: "the renewal loop stopped".to_owned(),
            }))
        })
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            let Self {
                lease,
                stop_work,
                stop_renewal,
                workers,
                renewal,
                ..
            } = *self;
            let grace = lease.config().drain_grace();
            stop_work.send_replace(true);
            for worker in workers {
                let abort = worker.abort_handle();
                if timeout(grace, worker).await.is_err() {
                    warn!(event = "lease.tick_cancelled", lease = %lease.name(), grace_ms = grace.as_millis(), "in-flight work did not finish within the drain grace; dropped");
                    abort.abort();
                }
            }
            stop_renewal.send_replace(true);
            let bound = lease.config().ttl();
            let abort = renewal.abort_handle();
            if timeout(bound, renewal).await.is_err() {
                warn!(event = "lease.release_failed", lease = %lease.name(), "the release did not finish; the lease expires by its TTL");
                abort.abort();
            }
            info!(event = "lease.surface_stopped", lease = %lease.name(), "leased surface stopped");
            Ok(())
        })
    }
}
