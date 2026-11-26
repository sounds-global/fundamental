//! [`PeriodicTask`]: what a [`PeriodicSurface`](crate::PeriodicSurface) runs on a tick.

use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;

/// One run of an idempotent periodic job: a sweep, a reconcile, a refresh.
pub trait PeriodicTask: Send + Sync + 'static {
    /// Runs the job once.
    ///
    /// # Errors
    /// [`AnyError`] if the run failed; the next tick retries.
    fn run(&self) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>>;
}
