//! `impl Surface for Worker` — runs a NATS worker as a supervised surface.

use std::future::pending;

use fundamental_core::AnyError;
use fundamental_nats::{Worker, WorkerGuard};

use crate::{DrainFuture, StartFuture, Surface, SurfaceGuard, WaitFuture};

const NAME: &str = "worker";

impl Surface for Worker {
    fn name(&self) -> &'static str {
        NAME
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let guard = self.serve();
            Ok(Box::new(WorkerSurfaceGuard(guard)) as Box<dyn SurfaceGuard>)
        })
    }
}

struct WorkerSurfaceGuard(WorkerGuard);

impl SurfaceGuard for WorkerSurfaceGuard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        Box::pin(pending::<Result<(), AnyError>>())
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        // Drained, not aborted: a queue-group member that aborts strands the requests already routed to it.
        Box::pin(async move { self.0.drain().await.map_err(AnyError::new) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A worker needs a live broker; this only pins that the impl exists and is object-safe.
    #[test]
    fn worker_is_a_surface() {
        fn assert_surface<T: Surface>() {}
        assert_surface::<Worker>();
    }
}
