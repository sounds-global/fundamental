//! [`PhasedSurface`]: places another surface in a chosen [`DrainPhase`].

use std::time::Duration;

use crate::{DrainPhase, StartFuture, Surface};

/// Wraps a [`Surface`] so it drains in a caller-chosen [`DrainPhase`].
///
///
/// ```rust,no_run
/// # extern crate fundamental_runtime;
/// # use fundamental_runtime::{DrainPhase, PhasedSurface, Runtime, Surface};
/// # fn wire(subscriber: impl Surface) -> Runtime {
/// // A NATS subscriber accepts outside work, so close it with the front door.
/// Runtime::new().with(PhasedSurface::new(DrainPhase::Intake, subscriber))
/// # }
/// ```
#[must_use = "a PhasedSurface does nothing until handed to a Runtime"]
pub struct PhasedSurface<S> {
    phase: DrainPhase,
    inner: S,
}

impl<S> PhasedSurface<S> {
    /// Wraps `inner` so the runtime drains it in `phase`.
    pub const fn new(phase: DrainPhase, inner: S) -> Self {
        Self { phase, inner }
    }
}

impl<S> Surface for PhasedSurface<S>
where
    S: Surface,
{
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn drain_phase(&self) -> DrainPhase {
        self.phase
    }

    /// Forwards the wrapped surface's drain budget.
    fn drain_budget(&self) -> Option<Duration> {
        self.inner.drain_budget()
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::new(self.inner).start()
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use fundamental_core::AnyError;

    use super::*;
    use crate::{DrainFuture, SurfaceGuard, WaitFuture};

    struct Plain;
    struct PlainGuard;

    impl Surface for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }

        fn start(self: Box<Self>) -> StartFuture {
            Box::pin(async move { Ok(Box::new(PlainGuard) as Box<dyn SurfaceGuard>) })
        }
    }

    impl SurfaceGuard for PlainGuard {
        fn name(&self) -> &'static str {
            "plain"
        }

        fn wait(&mut self) -> WaitFuture<'_> {
            Box::pin(pending::<Result<(), AnyError>>())
        }

        fn stop(self: Box<Self>) -> DrainFuture {
            Box::pin(async move { Ok(()) })
        }
    }

    #[test]
    fn the_phase_is_overridden_and_the_name_is_not() {
        assert_eq!(Plain.drain_phase(), DrainPhase::Work);

        let phased = PhasedSurface::new(DrainPhase::Observability, Plain);
        assert_eq!(phased.drain_phase(), DrainPhase::Observability);
        assert_eq!(
            phased.name(),
            "plain",
            "the wrapper moves a surface in the drain; it does not rename it"
        );
    }

    #[tokio::test]
    async fn the_wrapped_surface_still_starts_and_stops_itself() {
        let surface: Box<dyn Surface> = Box::new(PhasedSurface::new(DrainPhase::Intake, Plain));
        let guard = surface.start().await.expect("starts");
        assert_eq!(guard.name(), "plain");
        guard.stop().await.expect("stops");
    }
}
