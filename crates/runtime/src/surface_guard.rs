//! [`SurfaceGuard`]: a handle over one running surface.

use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;

/// The future returned by [`SurfaceGuard::wait`]: resolves when the surface ends on its own.
pub type WaitFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + 'a>>;

/// The future returned by [`SurfaceGuard::stop`]: resolves when the drain completes.
pub type DrainFuture = Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send>>;

/// A running surface's guard: the runtime watches it for a mid-run end and drains it on shutdown.
pub trait SurfaceGuard: Send + 'static {
    /// A short, stable name for logs.
    fn name(&self) -> &'static str;

    /// Resolves when the surface's own work ends before any shutdown signal; never, for one that only stops on shutdown.
    fn wait(&mut self) -> WaitFuture<'_>;

    /// Stops the surface, awaiting a clean drain.
    ///
    /// # Errors
    /// An [`AnyError`] if the drain fails.
    fn stop(self: Box<Self>) -> DrainFuture;
}
