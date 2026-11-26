use tokio::sync::oneshot;

/// The loop's half of a drain request: resolves once the guard asks the loop to finish.
///
/// Selected on beside the next delivery, so a drain is noticed only between messages.
#[derive(Debug)]
pub struct DrainRequested(Option<oneshot::Receiver<()>>);

impl DrainRequested {
    /// A connected pair: the guard keeps the sender, the loop takes this.
    pub(crate) fn pair() -> (oneshot::Sender<()>, Self) {
        let (sender, receiver) = oneshot::channel();
        (sender, Self(Some(receiver)))
    }

    /// Resolves when a drain is requested; never resolves otherwise.
    ///
    /// Cancel-safe. A sender dropped without asking is an abort, so that case waits forever.
    pub async fn arrived(&mut self) {
        let Some(receiver) = self.0.as_mut() else {
            return std::future::pending().await;
        };
        if receiver.await.is_err() {
            self.0 = None;
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn resolves_once_the_drain_is_asked_for() {
        let (sender, mut requested) = DrainRequested::pair();
        sender.send(()).expect("the loop's half is alive");
        tokio::time::timeout(Duration::from_secs(5), requested.arrived())
            .await
            .expect("a requested drain is seen");
    }

    /// A guard dropped without asking is an abort, not a drain.
    #[tokio::test(start_paused = true)]
    async fn a_dropped_sender_is_not_a_drain() {
        let (sender, mut requested) = DrainRequested::pair();
        drop(sender);
        assert!(
            tokio::time::timeout(Duration::from_secs(60), requested.arrived())
                .await
                .is_err(),
            "a dropped sender must never read as a drain request"
        );
    }
}
