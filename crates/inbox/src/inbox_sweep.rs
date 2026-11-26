//! [`InboxSweep`] — the inbox's sweep, as a periodic task on a runtime.

use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_runtime::PeriodicTask;

use crate::Inbox;

/// Runs [`Inbox::sweep`] on every tick of a
/// [`PeriodicSurface`](fundamental_runtime::PeriodicSurface).
#[derive(Clone, Debug)]
pub struct InboxSweep {
    inbox: Inbox,
}

impl InboxSweep {
    /// The sweep of `inbox`'s effects.
    #[must_use]
    pub fn new(inbox: Inbox) -> Self {
        Self { inbox }
    }
}

impl PeriodicTask for InboxSweep {
    fn run(&self) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>> {
        Box::pin(async move { self.inbox.sweep().await.map(|_| ()).map_err(AnyError::new) })
    }
}
