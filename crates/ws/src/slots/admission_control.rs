//! [`AdmissionControl`]: a server's slot pools, and how a new connection takes a slot.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::time::{Instant, sleep_until};
use tracing::{debug, warn};

use crate::WsConfig;
use crate::error::{WsError, WsResult};
use crate::observability::{ProcessRefusalBudget, WsStats};
use crate::server::Drain;
use crate::session::CloseReason;
use crate::slots::{
    AnonymousLimits, AnonymousRegistry, Displacement, Pool, Slot, SlotCell, SlotGuard,
    instant_after,
};

/// The disjoint anonymous, authenticated and over-share pools; the server holds at most
/// `max_connections + max_anonymous` sockets.
#[derive(Debug)]
pub(crate) struct AdmissionControl {
    limits: Option<AnonymousLimits>,
    anonymous: Arc<Semaphore>,
    authenticated: Arc<Semaphore>,
    refusing: Arc<Semaphore>,
    registry: AnonymousRegistry,
    stats: Arc<WsStats>,
    process_refusals: ProcessRefusalBudget,
    next_seq: AtomicU64,
    max_per_principal: usize,
    holders: Mutex<HashMap<String, usize>>,
    drain: Drain,
    lingering: Arc<Semaphore>,
}

impl AdmissionControl {
    /// Builds the pools a validated `config` declares.
    pub(crate) fn new(config: &WsConfig, now: Instant) -> Arc<Self> {
        let limits = config.admission.anonymous().cloned();
        let (anonymous, authenticated, stats) = match &limits {
            Some(limits) => {
                let anonymous = limits.capacity(config.max_connections);
                let authenticated = config.max_connections.saturating_sub(anonymous);
                (
                    anonymous,
                    authenticated,
                    WsStats::new(anonymous, authenticated, 0),
                )
            }
            None => (
                0,
                config.max_connections,
                WsStats::new(0, 0, config.max_connections),
            ),
        };
        Arc::new(Self {
            limits,
            anonymous: Arc::new(Semaphore::new(anonymous)),
            authenticated: Arc::new(Semaphore::new(authenticated)),
            refusing: Arc::new(Semaphore::new(anonymous)),
            registry: AnonymousRegistry::default(),
            stats: Arc::new(stats),
            process_refusals: ProcessRefusalBudget::new(now),
            next_seq: AtomicU64::new(0),
            max_per_principal: config.max_per_principal,
            holders: Mutex::new(HashMap::new()),
            drain: Drain::new(),
            lingering: Arc::new(Semaphore::new(config.max_connections)),
        })
    }

    /// Takes a slot for a newly accepted connection: a free anonymous slot, else displacement, else
    /// over the share; it waits only when the over-share pool is full too.
    ///
    /// # Errors
    /// [`WsError::Internal`] if a pool's semaphore was closed.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub(crate) async fn admit(self: &Arc<Self>) -> WsResult<SlotGuard> {
        let Some(limits) = &self.limits else {
            let permit = match Arc::clone(&self.authenticated).try_acquire_owned() {
                Ok(permit) => permit,
                Err(TryAcquireError::Closed) => return Err(pool_closed()),
                Err(TryAcquireError::NoPermits) => {
                    debug!("every public slot is taken; waiting for one");
                    acquire(&self.authenticated).await?
                }
            };
            return Ok(self.guard(Slot::Admitted {
                pool: Pool::Public,
                _permit: permit,
                holder: None,
            }));
        };
        loop {
            match self.anonymous.clone().try_acquire_owned() {
                Ok(permit) => return Ok(self.anonymous_guard(permit)),
                Err(TryAcquireError::Closed) => return Err(pool_closed()),
                Err(TryAcquireError::NoPermits) => {}
            }
            let now = Instant::now();
            let wake = match self.registry.displace_oldest(now, limits.displace_after) {
                Displacement::Displaced => {
                    let permit = acquire(&self.anonymous).await?;
                    return Ok(self.anonymous_guard(permit));
                }
                Displacement::NotBefore(at) => at,
                Displacement::Empty => instant_after(now, limits.displace_after),
            };
            if let Ok(permit) = self.refusing.clone().try_acquire_owned() {
                return Ok(self.over_share_guard(permit));
            }
            debug!("anonymous and over-share pools full; waiting for a slot");
            tokio::select! {
                permit = acquire(&self.anonymous) => return Ok(self.anonymous_guard(permit?)),
                permit = acquire(&self.refusing) => return Ok(self.over_share_guard(permit?)),
                () = sleep_until(wake) => {}
            }
        }
    }

    /// How long a client closed for `reason` should wait before reconnecting.
    pub(crate) fn retry_after(&self, reason: CloseReason) -> Option<Duration> {
        let limits = self.limits.as_ref()?;
        match reason {
            CloseReason::OverShare | CloseReason::Displaced => Some(limits.displace_after),
            CloseReason::Saturated => Some(limits.within),
            _ => None,
        }
    }

    pub(crate) fn limits(&self) -> Option<&AnonymousLimits> {
        self.limits.as_ref()
    }

    pub(crate) fn authenticated_permit(&self) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        self.authenticated.clone().try_acquire_owned()
    }

    pub(crate) fn refusing_permit(&self) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        self.refusing.clone().try_acquire_owned()
    }

    /// Counts one more connection for `holder`, returning `false` if it already holds `max_per_principal`.
    pub(crate) fn hold(&self, holder: &str) -> bool {
        let mut holders = self.holders();
        let held = holders.entry(holder.to_owned()).or_insert(0);
        if *held >= self.max_per_principal {
            return false;
        }
        *held += 1;
        true
    }

    pub(crate) fn release(&self, holder: &str) {
        let mut holders = self.holders();
        if let Some(held) = holders.get_mut(holder) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                holders.remove(holder);
            }
        }
    }

    pub(crate) fn registry(&self) -> &AnonymousRegistry {
        &self.registry
    }

    pub(crate) fn stats(&self) -> &WsStats {
        &self.stats
    }

    pub(crate) fn shared_stats(&self) -> Arc<WsStats> {
        Arc::clone(&self.stats)
    }

    pub(crate) fn process_refusals(&self) -> &ProcessRefusalBudget {
        &self.process_refusals
    }

    /// A place for a closed connection to wait for its peer's close, if one is free.
    pub(crate) fn linger_permit(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.lingering).try_acquire_owned().ok()
    }

    pub(crate) fn drain(&self) -> &Drain {
        &self.drain
    }

    fn anonymous_guard(self: &Arc<Self>, permit: OwnedSemaphorePermit) -> SlotGuard {
        let guard = self.guard(Slot::Anonymous(permit));
        let cell = guard.cell();
        self.registry
            .insert(cell.seq(), cell.admitted_at(), Arc::downgrade(cell));
        guard
    }

    fn over_share_guard(self: &Arc<Self>, permit: OwnedSemaphorePermit) -> SlotGuard {
        self.guard(Slot::Ending {
            reason: CloseReason::OverShare,
            pool: Pool::Refusing,
            _permit: permit,
        })
    }

    fn guard(self: &Arc<Self>, slot: Slot) -> SlotGuard {
        if let Some(pool) = slot.pool() {
            self.stats.opened(pool);
        }
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        SlotGuard::new(Arc::new(SlotCell::new(
            seq,
            Instant::now(),
            slot,
            Arc::clone(self),
        )))
    }
}

async fn acquire(pool: &Arc<Semaphore>) -> WsResult<OwnedSemaphorePermit> {
    Arc::clone(pool)
        .acquire_owned()
        .await
        .map_err(|_| pool_closed())
}

impl AdmissionControl {
    fn holders(&self) -> MutexGuard<'_, HashMap<String, usize>> {
        self.holders.lock().unwrap_or_else(|poisoned| {
            warn!("principal count lock was poisoned by a panic; recovering its counts");
            poisoned.into_inner()
        })
    }
}

fn pool_closed() -> WsError {
    WsError::internal("a connection slot pool was closed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    fn control(max_connections: usize, max_anonymous: usize) -> Arc<AdmissionControl> {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
            .max_connections(max_connections)
            .authenticate(AnonymousLimits::default().max_connections(max_anonymous));
        AdmissionControl::new(&config, Instant::now())
    }

    #[tokio::test]
    async fn a_newcomer_takes_a_free_anonymous_slot() {
        let control = control(4, 2);
        let guard = control.admit().await.expect("admitted");
        assert!(guard.cell().is_anonymous());
        assert_eq!(control.stats().anonymous(), 1);
        assert_eq!(control.registry().len(), 1);
    }

    #[tokio::test]
    async fn a_public_socket_admits_straight_to_its_one_pool() {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
            .max_connections(2)
            .public();
        let control = AdmissionControl::new(&config, Instant::now());
        let guard = control.admit().await.expect("admitted");
        assert!(!guard.cell().is_anonymous());
        assert_eq!(guard.cell().ending(), None);
        assert_eq!(control.stats().public(), 1);
        assert_eq!(control.registry().len(), 0);
        assert_eq!(control.retry_after(CloseReason::OverShare), None);
    }

    #[tokio::test]
    async fn a_newcomer_over_a_young_share_is_admitted_to_be_told_to_come_back() {
        let control = control(4, 1);
        let held = control.admit().await.expect("the one anonymous slot");
        let over = control.admit().await.expect("admitted over the share");
        assert_eq!(over.cell().ending(), Some(CloseReason::OverShare));
        assert_eq!(
            held.cell().ending(),
            None,
            "a young connection is never displaced"
        );
        assert_eq!(control.stats().refusing(), 1);
        assert_eq!(
            control.retry_after(CloseReason::OverShare),
            Some(AnonymousLimits::default().displace_after)
        );
    }

    #[tokio::test(start_paused = true)]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_full_over_share_pool_waits_for_the_oldest_to_cross_the_floor() {
        let control = control(4, 1);
        let oldest = control.admit().await.expect("anonymous");
        let _over = control.admit().await.expect("over the share");
        let waiting = tokio::spawn({
            let control = Arc::clone(&control);
            async move { control.admit().await }
        });
        tokio::task::yield_now().await;
        assert!(
            !waiting.is_finished(),
            "nothing is free and nothing is old enough"
        );
        tokio::time::advance(AnonymousLimits::default().displace_after).await;
        tokio::task::yield_now().await;
        assert_eq!(oldest.cell().ending(), Some(CloseReason::Displaced));
        let mut oldest = oldest;
        oldest.ended(CloseReason::Displaced);
        drop(oldest);
        let admitted = timeout(Duration::from_secs(1), waiting)
            .await
            .expect("admitted once the victim closed")
            .expect("task")
            .expect("admitted");
        assert!(admitted.cell().is_anonymous());
    }
}
