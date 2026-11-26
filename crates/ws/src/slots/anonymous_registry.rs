//! [`AnonymousRegistry`]: anonymous connections in the order they were admitted.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::time::Instant;
use tracing::warn;

use crate::slots::{Displacement, SlotCell, instant_after};

/// The anonymous connections a server holds, oldest first, by [`Weak`] reference; its lock never nests with a cell's.
#[derive(Debug, Default)]
pub(crate) struct AnonymousRegistry {
    entries: Mutex<BTreeMap<u64, (Instant, Weak<SlotCell>)>>,
}

impl AnonymousRegistry {
    pub(crate) fn insert(&self, seq: u64, admitted_at: Instant, cell: Weak<SlotCell>) {
        self.lock().insert(seq, (admitted_at, cell));
    }

    pub(crate) fn remove(&self, seq: u64) {
        self.lock().remove(&seq);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }

    /// Displaces the oldest anonymous connection admitted at least `floor` before `now`; the floor is what
    /// makes oldest-first safe.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn displace_oldest(&self, now: Instant, floor: Duration) -> Displacement {
        loop {
            let (admitted_at, cell) = {
                let mut entries = self.lock();
                let Some((&seq, &(admitted_at, _))) = entries.first_key_value() else {
                    return Displacement::Empty;
                };
                let eligible_at = instant_after(admitted_at, floor);
                if eligible_at > now {
                    return Displacement::NotBefore(eligible_at);
                }
                match entries.remove(&seq) {
                    Some(entry) => entry,
                    None => continue,
                }
            };
            if let Some(cell) = cell.upgrade()
                && cell.displace_if_anonymous()
            {
                let waited_ms = now.saturating_duration_since(admitted_at).as_millis();
                // Its line is a refusal's, within the server's budget: a flood of it is what an attacker causes.
                if cell.control().process_refusals().permit(now) {
                    warn!(
                        waited_ms = u64::try_from(waited_ms).unwrap_or(u64::MAX),
                        "anonymous share full: displacing the oldest unauthenticated connection"
                    );
                }
                return Displacement::Displaced;
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<u64, (Instant, Weak<SlotCell>)>> {
        self.entries.lock().unwrap_or_else(|poisoned| {
            warn!("anonymous registry lock was poisoned by a panic; recovering its entries");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::WsConfig;
    use crate::session::CloseReason;
    use crate::slots::{AdmissionControl, AnonymousLimits};

    const FLOOR: Duration = Duration::from_secs(5);

    fn control() -> Arc<AdmissionControl> {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
            .max_connections(10)
            .authenticate(AnonymousLimits::default().max_connections(3));
        AdmissionControl::new(&config, Instant::now())
    }

    #[tokio::test]
    async fn an_empty_registry_displaces_nobody() {
        let registry = AnonymousRegistry::default();
        assert_eq!(
            registry.displace_oldest(Instant::now(), FLOOR),
            Displacement::Empty
        );
    }

    #[tokio::test]
    async fn a_connection_younger_than_the_floor_is_never_displaced() {
        let control = control();
        let guard = control.admit().await.expect("admitted");
        let admitted_at = guard.cell().admitted_at();
        assert_eq!(
            control
                .registry()
                .displace_oldest(admitted_at + FLOOR - Duration::from_millis(1), FLOOR),
            Displacement::NotBefore(admitted_at + FLOOR)
        );
        assert_eq!(guard.cell().ending(), None);
    }

    #[tokio::test]
    async fn the_oldest_past_the_floor_is_displaced_first() {
        let control = control();
        let oldest = control.admit().await.expect("admitted");
        let newer = control.admit().await.expect("admitted");
        let now = newer.cell().admitted_at() + FLOOR;
        assert_eq!(
            control.registry().displace_oldest(now, FLOOR),
            Displacement::Displaced
        );
        assert_eq!(oldest.cell().ending(), Some(CloseReason::Displaced));
        assert_eq!(newer.cell().ending(), None);
        assert_eq!(control.registry().len(), 1);
    }

    #[tokio::test]
    async fn a_connection_that_bound_is_passed_over() {
        let control = control();
        let bound = control.admit().await.expect("admitted");
        let anonymous = control.admit().await.expect("admitted");
        bound.cell().bind("holder", 1u32).expect("bound");
        let now = anonymous.cell().admitted_at() + FLOOR;
        assert_eq!(
            control.registry().displace_oldest(now, FLOOR),
            Displacement::Displaced
        );
        assert_eq!(bound.cell().ending(), None);
        assert_eq!(anonymous.cell().ending(), Some(CloseReason::Displaced));
    }
}
