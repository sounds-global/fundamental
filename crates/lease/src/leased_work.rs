use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;

use crate::HeldLease;

/// Singleton work a [`LeasedSurface`](crate::LeasedSurface) runs only while this process holds the lease.
///
/// Every write goes through [`HeldLease::transaction`], so a tick that outlives its tenure cannot commit.
pub trait LeasedWork: Send + Sync + 'static {
    /// Runs one tick while holding `lease`.
    ///
    /// # Errors
    ///
    /// The work's own failure; the surface logs it and runs the next tick.
    fn run<'a>(
        &'a self,
        lease: &'a HeldLease,
    ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + 'a>>;
}
