use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use fundamental_core::AnyError;
use fundamental_runtime::{
    DrainFuture, DrainPhase, StartFuture, Surface, SurfaceGuard, WaitFuture,
};

/// The future that builds a deferred surface.
pub(crate) type BuildFuture = Pin<Box<dyn Future<Output = Result<Built, AnyError>> + Send>>;

/// A deferred surface, built, and what it needs kept alive while it runs.
pub(crate) struct Built {
    surface: Box<dyn Surface>,
    held: Option<Box<dyn Send>>,
}

impl Built {
    /// `surface`, needing nothing else kept.
    pub(crate) fn new(surface: impl Surface) -> Self {
        Self {
            surface: Box::new(surface),
            held: None,
        }
    }

    /// Keeps `held` alive until the surface has stopped.
    #[cfg_attr(
        not(all(feature = "sync", feature = "cql")),
        expect(dead_code, reason = "a cql store's connections")
    )]
    pub(crate) fn holding(mut self, held: impl Send + 'static) -> Self {
        self.held = Some(Box::new(held));
        self
    }
}

/// A surface whose building does I/O, built only when the runtime starts it.
pub(crate) struct Deferred {
    name: &'static str,
    phase: DrainPhase,
    budget: Option<Duration>,
    build: Box<dyn FnOnce() -> BuildFuture + Send>,
}

impl Deferred {
    /// The surface `name`, drained in `phase` within `budget`, that `build` makes.
    pub(crate) fn new(
        name: &'static str,
        phase: DrainPhase,
        budget: Option<Duration>,
        build: impl FnOnce() -> BuildFuture + Send + 'static,
    ) -> Self {
        Self {
            name,
            phase,
            budget,
            build: Box::new(build),
        }
    }
}

impl Surface for Deferred {
    fn name(&self) -> &'static str {
        self.name
    }

    fn drain_phase(&self) -> DrainPhase {
        self.phase
    }

    fn drain_budget(&self) -> Option<Duration> {
        self.budget
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let built = (self.build)().await?;
            let guard = built.surface.start().await?;
            Ok(Box::new(Holding {
                guard,
                held: built.held,
            }) as Box<dyn SurfaceGuard>)
        })
    }
}

/// A running surface's guard, and what it needs kept alive until it has stopped.
struct Holding {
    guard: Box<dyn SurfaceGuard>,
    held: Option<Box<dyn Send>>,
}

impl SurfaceGuard for Holding {
    fn name(&self) -> &'static str {
        self.guard.name()
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        self.guard.wait()
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            let stopped = self.guard.stop().await;
            drop(self.held);
            stopped
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused() -> BuildFuture {
        Box::pin(async { Err(AnyError::new(crate::AppError::Stateless)) })
    }

    #[tokio::test]
    async fn a_deferred_surface_that_cannot_be_built_does_not_start() {
        let surface = Box::new(Deferred::new("worker", DrainPhase::Work, None, refused));
        assert_eq!(surface.name(), "worker");
        assert_eq!(surface.drain_phase(), DrainPhase::Work);
        let Err(refused) = surface.start().await else {
            panic!("a surface that cannot be built is refused");
        };
        assert_eq!(refused.get().error_code(), "SERVE_002");
    }
}
