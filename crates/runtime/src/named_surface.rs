//! [`NamedSurface`]: renames another surface for the supervision log.

use std::time::Duration;

use crate::{DrainFuture, DrainPhase, StartFuture, Surface, SurfaceGuard, WaitFuture};

/// Wraps a [`Surface`] so it reports a caller-chosen name in the runtime's start and drain lines.
///
/// ```rust,no_run
/// # extern crate fundamental_runtime;
/// # use fundamental_runtime::{NamedSurface, Runtime, Surface};
/// # fn wire(internal: impl Surface, public: impl Surface) -> Runtime {
/// Runtime::new()
///     .with(NamedSurface::new("api-internal", internal))
///     .with(NamedSurface::new("api-public", public))
/// # }
/// ```
#[must_use = "a NamedSurface does nothing until handed to a Runtime"]
pub struct NamedSurface<S> {
    name: &'static str,
    inner: S,
}

impl<S> NamedSurface<S> {
    /// Wraps `inner`, reporting it as `name`.
    pub const fn new(name: &'static str, inner: S) -> Self {
        Self { name, inner }
    }
}

impl<S> Surface for NamedSurface<S>
where
    S: Surface,
{
    fn name(&self) -> &'static str {
        self.name
    }

    /// Forwards the wrapped surface's phase, so a rename never moves it in the drain.
    fn drain_phase(&self) -> DrainPhase {
        self.inner.drain_phase()
    }

    /// Forwards the wrapped surface's drain budget.
    fn drain_budget(&self) -> Option<Duration> {
        self.inner.drain_budget()
    }

    fn start(self: Box<Self>) -> StartFuture {
        let Self { name, inner } = *self;
        Box::pin(async move {
            let inner = Box::new(inner).start().await?;
            Ok(Box::new(NamedGuard { name, inner }) as Box<dyn SurfaceGuard>)
        })
    }
}

/// The guard carries the name too: the runtime logs the drain from the guard.
struct NamedGuard {
    name: &'static str,
    inner: Box<dyn SurfaceGuard>,
}

impl SurfaceGuard for NamedGuard {
    fn name(&self) -> &'static str {
        self.name
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        self.inner.wait()
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        self.inner.stop()
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::{Arc, Mutex};

    use fundamental_core::AnyError;

    use super::*;

    struct Recording {
        stopped_as: Arc<Mutex<Option<&'static str>>>,
    }

    struct RecordingGuard {
        name: &'static str,
        stopped_as: Arc<Mutex<Option<&'static str>>>,
    }

    impl Surface for Recording {
        fn name(&self) -> &'static str {
            "inner"
        }

        fn start(self: Box<Self>) -> StartFuture {
            Box::pin(async move {
                Ok(Box::new(RecordingGuard {
                    name: "inner",
                    stopped_as: self.stopped_as,
                }) as Box<dyn SurfaceGuard>)
            })
        }
    }

    impl SurfaceGuard for RecordingGuard {
        fn name(&self) -> &'static str {
            self.name
        }

        fn wait(&mut self) -> WaitFuture<'_> {
            Box::pin(pending::<Result<(), AnyError>>())
        }

        fn stop(self: Box<Self>) -> DrainFuture {
            Box::pin(async move {
                *self.stopped_as.lock().expect("stopped_as lock") = Some(self.name);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn the_name_is_overridden_on_both_the_surface_and_its_guard() {
        let stopped_as = Arc::new(Mutex::new(None));
        let surface: Box<dyn Surface> = Box::new(NamedSurface::new(
            "admin",
            Recording {
                stopped_as: stopped_as.clone(),
            },
        ));
        assert_eq!(surface.name(), "admin");

        let guard = surface.start().await.expect("starts");
        assert_eq!(
            guard.name(),
            "admin",
            "the drain is logged from the guard, so the rename must reach it too"
        );
        guard.stop().await.expect("stops");

        assert_eq!(*stopped_as.lock().expect("stopped_as lock"), Some("inner"));
    }
}
