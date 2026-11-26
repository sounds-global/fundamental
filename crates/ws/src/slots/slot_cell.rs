//! [`SlotCell`]: one connection's slot, its bound principal and its refusals.

use std::any::Any;
use std::mem;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::Notify;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::context::connection_state::ConnectionState;
use crate::error::{WsError, WsResult};
use crate::observability::ConnectionRefusals;
use crate::session::CloseReason;
use crate::slots::{AdmissionControl, Pool, Slot};

/// One connection's admission; binding and displacement take the slot's one lock, so they are exclusive.
#[derive(Debug)]
pub(crate) struct SlotCell {
    seq: u64,
    admitted_at: Instant,
    slot: Mutex<Slot>,
    changed: Notify,
    principal: ConnectionState,
    refusals: ConnectionRefusals,
    control: Arc<AdmissionControl>,
}

impl SlotCell {
    pub(crate) fn new(
        seq: u64,
        admitted_at: Instant,
        slot: Slot,
        control: Arc<AdmissionControl>,
    ) -> Self {
        Self {
            seq,
            admitted_at,
            slot: Mutex::new(slot),
            changed: Notify::new(),
            principal: ConnectionState::default(),
            refusals: ConnectionRefusals::new(admitted_at),
            control,
        }
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    pub(crate) fn admitted_at(&self) -> Instant {
        self.admitted_at
    }

    pub(crate) fn control(&self) -> &AdmissionControl {
        &self.control
    }

    /// Binds `principal` against `holder`'s count and moves the slot to the authenticated pool.
    pub(crate) fn bind<P: Any + Send + Sync>(&self, holder: &str, principal: P) -> WsResult<()> {
        let mut slot = self.lock();
        if self.principal.is_set() {
            return Err(WsError::AlreadyAuthenticated);
        }
        let was_anonymous = match &*slot {
            Slot::Anonymous(_) => true,
            Slot::Admitted { .. } => false,
            Slot::Ending { .. } | Slot::Closed => return Err(WsError::Closed),
        };
        if !self.control.hold(holder) {
            self.end(&mut slot, CloseReason::PrincipalLimit);
            drop(slot);
            self.changed.notify_one();
            return Err(WsError::PrincipalLimit);
        }
        let stats = self.control.stats();
        match mem::replace(&mut *slot, Slot::Closed) {
            Slot::Anonymous(anonymous) => match self.control.authenticated_permit() {
                Ok(permit) => {
                    *slot = Slot::Admitted {
                        pool: Pool::Authenticated,
                        _permit: permit,
                        holder: Some(holder.to_owned()),
                    };
                    drop(anonymous);
                    stats.released(Pool::Anonymous);
                    stats.opened(Pool::Authenticated);
                }
                Err(_) => {
                    *slot = Slot::Anonymous(anonymous);
                    self.control.release(holder);
                    self.end(&mut slot, CloseReason::Saturated);
                    drop(slot);
                    self.changed.notify_one();
                    return Err(WsError::Saturated);
                }
            },
            Slot::Admitted { pool, _permit, .. } => {
                *slot = Slot::Admitted {
                    pool,
                    _permit,
                    holder: Some(holder.to_owned()),
                };
            }
            other => {
                *slot = other;
                self.control.release(holder);
                return Err(WsError::Closed);
            }
        }
        self.store(principal)?;
        drop(slot);
        if was_anonymous {
            self.control.registry().remove(self.seq);
        }
        debug!("principal bound");
        Ok(())
    }

    /// Tells an anonymous connection to end because its slot is wanted, returning whether it was anonymous;
    /// the anonymous slot is handed back at once unless the over-share pool is full.
    pub(crate) fn displace_if_anonymous(&self) -> bool {
        let mut slot = self.lock();
        let Slot::Anonymous(permit) = mem::replace(&mut *slot, Slot::Closed) else {
            return false;
        };
        let stats = self.control.stats();
        *slot = match self.control.refusing_permit() {
            Ok(refusing) => {
                drop(permit);
                stats.released(Pool::Anonymous);
                stats.opened(Pool::Refusing);
                Slot::Ending {
                    reason: CloseReason::Displaced,
                    pool: Pool::Refusing,
                    _permit: refusing,
                }
            }
            Err(_) => Slot::Ending {
                reason: CloseReason::Displaced,
                pool: Pool::Anonymous,
                _permit: permit,
            },
        };
        drop(slot);
        self.changed.notify_one();
        true
    }

    /// Gives the permit and the holder's count back and counts the close.
    pub(crate) fn close(&self, reason: CloseReason) {
        let released = mem::replace(&mut *self.lock(), Slot::Closed);
        let stats = self.control.stats();
        if let Some(pool) = released.pool() {
            stats.released(pool);
        }
        if let Slot::Admitted {
            holder: Some(holder),
            ..
        } = &released
        {
            self.control.release(holder);
        }
        drop(released);
        stats.closed(reason);
        self.control.registry().remove(self.seq);
    }

    fn end(&self, slot: &mut Slot, reason: CloseReason) {
        *slot = match mem::replace(slot, Slot::Closed) {
            Slot::Anonymous(permit) => Slot::Ending {
                reason,
                pool: Pool::Anonymous,
                _permit: permit,
            },
            Slot::Admitted { pool, _permit, .. } => Slot::Ending {
                reason,
                pool,
                _permit,
            },
            ending => ending,
        };
    }

    pub(crate) fn ending(&self) -> Option<CloseReason> {
        match &*self.lock() {
            Slot::Ending { reason, .. } => Some(*reason),
            _ => None,
        }
    }

    pub(crate) fn is_anonymous(&self) -> bool {
        matches!(&*self.lock(), Slot::Anonymous(_))
    }

    pub(crate) async fn changed(&self) {
        self.changed.notified().await;
    }

    pub(crate) fn principal<P: Any + Send + Sync>(&self) -> Option<&P> {
        self.principal.get::<P>()
    }

    pub(crate) fn has_principal(&self) -> bool {
        self.principal.is_set()
    }

    /// Counts one refused frame, returning whether its warning may be written.
    pub(crate) fn count_refusal(&self) -> bool {
        self.refusals.count(
            Instant::now(),
            self.control.process_refusals(),
            self.control.stats(),
        )
    }

    pub(crate) fn refusals(&self) -> &ConnectionRefusals {
        &self.refusals
    }

    fn store<P: Any + Send + Sync>(&self, principal: P) -> WsResult<()> {
        self.principal
            .set(principal)
            .map_err(|_| WsError::AlreadyAuthenticated)
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|poisoned| {
            warn!("connection slot lock was poisoned by a panic; recovering its state");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WsConfig;
    use crate::slots::{AnonymousLimits, SlotGuard};
    use fundamental_core::Error as _;

    fn control(max_connections: usize, max_anonymous: usize) -> Arc<AdmissionControl> {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
            .max_connections(max_connections)
            .authenticate(AnonymousLimits::default().max_connections(max_anonymous));
        AdmissionControl::new(&config, Instant::now())
    }

    async fn anonymous_cell(control: &Arc<AdmissionControl>) -> (Arc<SlotCell>, SlotGuard) {
        let guard = control.admit().await.expect("admitted");
        (Arc::clone(guard.cell()), guard)
    }

    #[tokio::test]
    async fn binding_moves_the_slot_to_the_authenticated_pool() {
        let control = control(4, 2);
        let (cell, _cell_guard) = anonymous_cell(&control).await;
        assert_eq!(control.stats().anonymous(), 1);
        cell.bind("holder", 7u32).expect("bound");
        assert_eq!(control.stats().anonymous(), 0);
        assert_eq!(control.stats().authenticated(), 1);
        assert_eq!(cell.principal::<u32>(), Some(&7));
        assert!(!cell.is_anonymous());
    }

    #[tokio::test]
    async fn a_second_bind_is_refused() {
        let control = control(4, 2);
        let (cell, _cell_guard) = anonymous_cell(&control).await;
        cell.bind("holder", 1u32).expect("first bind");
        let refused = cell.bind("holder", 2u32).expect_err("second bind");
        assert!(matches!(refused, WsError::AlreadyAuthenticated));
        assert_eq!(refused.error_code(), "WS_009");
        assert_eq!(cell.principal::<u32>(), Some(&1));
    }

    #[tokio::test]
    async fn displacement_never_closes_an_authenticated_connection() {
        let control = control(4, 2);
        let (cell, _cell_guard) = anonymous_cell(&control).await;
        cell.bind("holder", 1u32).expect("bound");
        assert!(!cell.displace_if_anonymous());
        assert_eq!(cell.ending(), None);
        assert_eq!(control.stats().authenticated(), 1);
    }

    #[tokio::test]
    async fn a_displaced_connection_cannot_bind() {
        let control = control(4, 2);
        let (cell, _cell_guard) = anonymous_cell(&control).await;
        assert!(cell.displace_if_anonymous());
        assert_eq!(cell.ending(), Some(CloseReason::Displaced));
        let refused = cell.bind("holder", 1u32).expect_err("displaced");
        assert!(matches!(refused, WsError::Closed));
        assert!(!cell.has_principal());
    }

    #[tokio::test]
    async fn a_principal_cannot_hold_more_than_its_share_of_connections() {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
            .max_connections(10)
            .max_per_principal(2)
            .authenticate(AnonymousLimits::default().max_connections(5));
        let control = AdmissionControl::new(&config, Instant::now());
        let (first, _first_guard) = anonymous_cell(&control).await;
        let (second, _second_guard) = anonymous_cell(&control).await;
        let (third, _third_guard) = anonymous_cell(&control).await;
        first.bind("alice", 1u32).expect("first");
        second.bind("alice", 2u32).expect("second");
        let refused = third.bind("alice", 3u32).expect_err("a third for alice");
        assert!(matches!(refused, WsError::PrincipalLimit));
        assert_eq!(refused.error_code(), "WS_013");
        assert_eq!(third.ending(), Some(CloseReason::PrincipalLimit));
        assert_eq!(control.stats().authenticated(), 2);

        first.close(CloseReason::ClientClosed);
        let (fourth, _fourth_guard) = anonymous_cell(&control).await;
        fourth
            .bind("alice", 4u32)
            .expect("a closed connection gives its count back");
    }

    #[tokio::test]
    async fn a_displaced_connection_hands_its_anonymous_slot_back_at_once() {
        let control = control(4, 1);
        let (cell, _guard) = anonymous_cell(&control).await;
        assert!(cell.displace_if_anonymous());
        assert_eq!(control.stats().anonymous(), 0);
        assert_eq!(control.stats().refusing(), 1);
        let newcomer = tokio::time::timeout(std::time::Duration::from_millis(50), control.admit())
            .await
            .expect("the slot is free before the victim closes")
            .expect("admitted");
        assert!(newcomer.cell().is_anonymous());
    }

    #[tokio::test]
    async fn a_full_authenticated_pool_refuses_the_bind_and_ends_the_connection() {
        let control = control(2, 1);
        let (first, _first_guard) = anonymous_cell(&control).await;
        first
            .bind("holder", 1u32)
            .expect("the one authenticated slot");
        let (second, _second_guard) = anonymous_cell(&control).await;
        let refused = second
            .bind("holder", 2u32)
            .expect_err("no authenticated slot");
        assert!(matches!(refused, WsError::Saturated));
        assert_eq!(refused.error_code(), "WS_008");
        assert_eq!(second.ending(), Some(CloseReason::Saturated));
        assert!(!second.has_principal());
        assert_eq!(
            control.stats().anonymous(),
            1,
            "it keeps its permit until it closes"
        );
    }

    #[tokio::test]
    async fn closing_gives_the_permit_back_and_counts_the_reason() {
        let control = control(4, 2);
        let (cell, _cell_guard) = anonymous_cell(&control).await;
        cell.close(CloseReason::AuthDeadline);
        assert_eq!(control.stats().anonymous(), 0);
        assert_eq!(control.stats().closes(CloseReason::AuthDeadline), 1);
        cell.close(CloseReason::AuthDeadline);
        assert_eq!(
            control.stats().anonymous(),
            0,
            "a second close releases nothing"
        );
    }
}
