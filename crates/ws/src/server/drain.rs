//! [`Drain`]: the one drain state a server's sessions all watch.

use std::sync::OnceLock;
use std::time::Duration;

use fundamental_core::model::Identifier;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::warn;

use crate::server::drain_state::DrainState;
use crate::session::ConnectionId;

/// The low 62 random bits of a v4 id, below the variant bits.
const TURN_BITS: u32 = 62;

/// A `watch`, so a session that starts watching after the drain began sees it at once.
#[derive(Debug)]
pub(crate) struct Drain {
    state: watch::Sender<DrainState>,
    began: OnceLock<(Instant, Duration)>,
}

impl Drain {
    pub(crate) fn new() -> Self {
        Self {
            state: watch::Sender::new(DrainState::Serving),
            began: OnceLock::new(),
        }
    }

    /// Tells every session to end at its next frame boundary, each within `spread` of now.
    pub(crate) fn begin(&self, spread: Duration) {
        if self.began.set((Instant::now(), spread)).is_err() {
            warn!("the drain was begun twice; keeping its first spread");
        }
        self.advance(DrainState::Draining);
    }

    /// When connection `id` should close: uniform across the spread by the id's random bits, so a hub's
    /// population redials its neighbours at a steady rate.
    pub(crate) fn turn(&self, id: &ConnectionId) -> Instant {
        let Some((began, spread)) = self.began.get() else {
            return Instant::now();
        };
        let draw = id.as_inner().as_u128() & ((1 << TURN_BITS) - 1);
        let offset = (spread.as_nanos().saturating_mul(draw)) >> TURN_BITS;
        let offset = Duration::from_nanos(u64::try_from(offset).unwrap_or(u64::MAX));
        *began + offset
    }

    /// Marks the sessions about to be aborted, so each counts as a drain timeout, not a failure.
    pub(crate) fn expire(&self) {
        self.advance(DrainState::Expired);
    }

    pub(crate) fn is_expired(&self) -> bool {
        *self.state.borrow() == DrainState::Expired
    }

    pub(crate) fn begun(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.state.subscribe();
        async move {
            // The sender outlives every session; if the channel ever closed, ending is the safe answer.
            if let Err(error) = state.wait_for(|state| *state >= DrainState::Draining).await {
                warn!(%error, "the drain state closed under a session; ending it");
            }
        }
    }

    fn advance(&self, to: DrainState) {
        self.state.send_if_modified(|state| {
            let forward = *state < to;
            if forward {
                *state = to;
            }
            forward
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn a_session_that_watches_after_the_drain_began_sees_it_at_once() {
        let drain = Drain::new();
        drain.begin(Duration::ZERO);
        timeout(Duration::from_secs(1), drain.begun())
            .await
            .expect("already begun");
    }

    #[tokio::test]
    async fn a_waiting_session_is_woken_when_the_drain_begins() {
        let drain = Drain::new();
        let begun = tokio::spawn(drain.begun());
        tokio::task::yield_now().await;
        assert!(!begun.is_finished());
        drain.begin(Duration::ZERO);
        timeout(Duration::from_secs(1), begun)
            .await
            .expect("woken")
            .expect("not panicked");
    }

    #[test]
    fn every_turn_falls_within_the_spread_and_the_turns_spread_across_it() {
        use fundamental_core::GeneratedId;
        let drain = Drain::new();
        let spread = Duration::from_secs(10);
        drain.begin(spread);
        let (began, _) = *drain.began.get().expect("begun");
        let offsets: Vec<Duration> = (0..1_000)
            .map(|_| drain.turn(&ConnectionId::generate()) - began)
            .collect();
        assert!(offsets.iter().all(|offset| *offset <= spread));
        let early = offsets.iter().filter(|o| **o < spread / 10).count();
        let late = offsets.iter().filter(|o| **o >= spread * 9 / 10).count();
        assert!(
            (50..=150).contains(&early),
            "{early} of 1000 in the first tenth"
        );
        assert!(
            (50..=150).contains(&late),
            "{late} of 1000 in the last tenth"
        );
    }

    #[test]
    fn a_turn_before_the_drain_is_now() {
        use fundamental_core::GeneratedId;
        let drain = Drain::new();
        let before = Instant::now();
        assert!(drain.turn(&ConnectionId::generate()) >= before);
    }

    #[test]
    fn the_state_only_moves_forward() {
        let drain = Drain::new();
        drain.expire();
        drain.begin(Duration::ZERO);
        assert!(drain.is_expired());
    }
}
