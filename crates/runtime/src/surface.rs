//! [`Surface`]: a bootable application surface the runtime can supervise.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use fundamental_core::AnyError;

use crate::{DrainPhase, SurfaceGuard};

/// The future returned by [`Surface::start`]: the running surface's guard, or its error.
pub type StartFuture =
    Pin<Box<dyn Future<Output = Result<Box<dyn SurfaceGuard>, AnyError>> + Send>>;

/// A bootable application surface (a NATS worker, a WebSocket server, an HTTP API), started once into a [`SurfaceGuard`].
pub trait Surface: Send + 'static {
    /// A short, stable name for logs (for example `"worker"` or `"socket"`).
    fn name(&self) -> &'static str;

    /// When this surface is stopped, relative to the others.
    fn drain_phase(&self) -> DrainPhase {
        DrainPhase::Work
    }

    /// The longest this surface's [`SurfaceGuard::stop`] can take, if it bounds its own drain.
    ///
    /// [`Runtime::run`](crate::Runtime::run) refuses to start when it does not fit the shutdown budget.
    fn drain_budget(&self) -> Option<Duration> {
        None
    }

    /// Boots the surface, returning a guard over its running task.
    ///
    /// An [`AnyError`] wrapping the surface's own error when it cannot start.
    fn start(self: Box<Self>) -> StartFuture;
}
