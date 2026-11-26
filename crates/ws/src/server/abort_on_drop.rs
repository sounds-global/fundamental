//! [`AbortOnDrop`]: ties a helper task's life to the task that spawned it.

use tokio::task::JoinHandle;

/// Aborts the task it holds when dropped, so a helper never outlives the accept loop.
#[derive(Debug)]
pub(crate) struct AbortOnDrop(JoinHandle<()>);

impl AbortOnDrop {
    pub(crate) fn new(handle: JoinHandle<()>) -> Self {
        Self(handle)
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    #[tokio::test]
    async fn dropping_the_guard_aborts_the_task() {
        let handle = tokio::spawn(pending::<()>());
        let abort = handle.abort_handle();
        drop(AbortOnDrop::new(handle));
        tokio::task::yield_now().await;
        assert!(abort.is_finished());
    }
}
