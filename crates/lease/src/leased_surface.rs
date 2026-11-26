use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;

use fundamental_core::{AnyError, Error as _};
use fundamental_runtime::{StartFuture, Surface, SurfaceGuard};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior, interval, sleep_until, timeout};
use tracing::{Instrument as _, Span, debug, error, info, warn};

use crate::leased_guard::LeasedGuard;
use crate::{Acquisition, HeldLease, Lease, LeaseObserver, LeaseState, LeasedWork, LossReason};

/// How often a passive instance says it is passive, so it never looks like a dead one.
const PASSIVE_HEARTBEAT: Duration = Duration::from_secs(60);

/// How often the holder prunes work keys past their retention window.
const PRUNE_EVERY: Duration = Duration::from_secs(3_600);

/// A runtime surface that runs singleton work only while this process holds its lease.
///
/// A tick still running when the lease is lost is dropped, rolling back its transaction.
pub struct LeasedSurface {
    lease: Lease,
    works: Vec<Registration>,
    observer: Option<Arc<dyn LeaseObserver>>,
}

struct Registration {
    name: &'static str,
    work: Arc<dyn LeasedWork>,
    every: Duration,
}

impl LeasedSurface {
    /// A surface for `lease`, with no work yet.
    #[must_use]
    pub fn new(lease: Lease) -> Self {
        Self {
            lease,
            works: Vec::new(),
            observer: None,
        }
    }

    /// Runs `work` every `every` while the lease is held.
    #[must_use]
    pub fn with_work(
        mut self,
        name: &'static str,
        work: Arc<dyn LeasedWork>,
        every: Duration,
    ) -> Self {
        self.works.push(Registration { name, work, every });
        self
    }

    /// Reports each tenure begun and lost to `observer`.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn LeaseObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl Surface for LeasedSurface {
    fn name(&self) -> &'static str {
        self.lease.name().as_str()
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let Self {
                lease,
                works,
                observer,
            } = *self;
            // A missing table fails boot here rather than warning on every renewal.
            lease.current().await.map_err(AnyError::new)?;
            let (held_tx, held_rx) = watch::channel::<Option<HeldLease>>(None);
            let (stop_work, stop_work_rx) = watch::channel(false);
            let (stop_renewal, stop_renewal_rx) = watch::channel(false);
            let (ended_tx, ended_rx) = watch::channel(false);
            let names: Vec<&str> = works.iter().map(|registration| registration.name).collect();
            info!(
                event = "lease.surface_started",
                lease = %lease.name(),
                holder = %lease.holder(),
                ttl_ms = lease.config().ttl().as_millis(),
                renew_every_ms = lease.config().renew_every().as_millis(),
                works = ?names,
                "leased surface started"
            );
            let span = Span::current();
            let workers = works
                .into_iter()
                .map(|registration| {
                    tokio::spawn(
                        work_loop(
                            lease.clone(),
                            registration,
                            held_rx.clone(),
                            stop_work_rx.clone(),
                        )
                        .instrument(span.clone()),
                    )
                })
                .collect();
            let renewal = tokio::spawn(
                renew(lease.clone(), held_tx, stop_renewal_rx, observer, ended_tx).instrument(span),
            );
            Ok(Box::new(LeasedGuard::new(
                lease,
                stop_work,
                stop_renewal,
                ended_rx,
                workers,
                renewal,
            )) as Box<dyn SurfaceGuard>)
        })
    }
}

/// Takes, keeps and finally releases the lease, publishing the current proof for the work loops.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn renew(
    lease: Lease,
    held: watch::Sender<Option<HeldLease>>,
    mut stop: watch::Receiver<bool>,
    observer: Option<Arc<dyn LeaseObserver>>,
    ended: watch::Sender<bool>,
) {
    let name = lease.name();
    let me = lease.holder().as_uuid();
    let mut ticker = interval(lease.config().renew_every());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut passive_logged: Option<Instant> = None;
    let mut pruned_at: Option<Instant> = None;
    let lose = |reason: LossReason| {
        if let Some(previous) = held.send_replace(None) {
            warn!(event = "lease.lost", lease = %name, epoch = previous.epoch(), ?reason, "lease lost");
            if let Some(observer) = &observer {
                observer.lost(name, previous.epoch(), reason);
            }
        }
    };
    loop {
        let deadline = held.borrow().as_ref().map(HeldLease::deadline);
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
                continue;
            }
            () = sleep_until(deadline.unwrap_or_else(far_future)), if deadline.is_some() => {
                lose(LossReason::Expired);
                continue;
            }
            _ = ticker.tick() => {}
        }
        if held.borrow().is_none() {
            let Ok(probe) = timeout(lease.config().ttl(), lease.current()).await else {
                warn!(event = "lease.store_unavailable", lease = %name, "a lease probe took longer than the TTL; abandoned");
                continue;
            };
            match probe {
                Ok((
                    Some(LeaseState::Held {
                        holder,
                        epoch,
                        expires_at,
                        ..
                    }),
                    now,
                )) if holder != me && expires_at > now => {
                    if passive_logged.is_none_or(|at| at.elapsed() >= PASSIVE_HEARTBEAT) {
                        info!(event = "lease.passive", lease = %name, %holder, epoch, %expires_at, "another instance holds the lease; this one is passive");
                        passive_logged = Some(Instant::now());
                    }
                    continue;
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(event = "lease.store_unavailable", lease = %name, code = error.error_code(), %error, "lease store did not answer; trying again next renewal");
                    continue;
                }
            }
        }
        // A store that hangs must not keep this process believing it holds the lease.
        let deadline = held.borrow().as_ref().map(HeldLease::deadline);
        let attempt = tokio::select! {
            biased;
            () = sleep_until(deadline.unwrap_or_else(far_future)), if deadline.is_some() => {
                lose(LossReason::Expired);
                continue;
            }
            attempt = timeout(lease.config().ttl(), lease.try_acquire()) => attempt,
        };
        let Ok(attempt) = attempt else {
            warn!(event = "lease.store_unavailable", lease = %name, "a lease renewal took longer than the TTL; abandoned");
            continue;
        };
        match attempt {
            Ok(Acquisition::Held(proof)) => {
                // A tenure given up locally is announced again on resume, so no work runs under an ended one.
                let resumed = held.borrow().is_none();
                if proof.began() || resumed {
                    info!(
                        event = "lease.acquired",
                        lease = %name,
                        epoch = proof.epoch(),
                        handover_gap_ms = proof.handover_gap().map(|gap| gap.as_millis()),
                        "lease tenure began"
                    );
                    if let Some(observer) = &observer {
                        observer.acquired(name, proof.epoch(), proof.handover_gap());
                    }
                } else {
                    debug!(event = "lease.renewed", lease = %name, epoch = proof.epoch(), "lease renewed");
                }
                passive_logged = None;
                if pruned_at.is_none_or(|at| at.elapsed() >= PRUNE_EVERY) {
                    pruned_at = Some(Instant::now());
                    prune(&lease, &proof).await;
                }
                held.send_replace(Some(proof.watched(held.subscribe())));
            }
            Ok(Acquisition::HeldElsewhere { epoch, expires_at }) => {
                lose(LossReason::Superseded);
                if passive_logged.is_none_or(|at| at.elapsed() >= PASSIVE_HEARTBEAT) {
                    info!(event = "lease.passive", lease = %name, epoch, %expires_at, "another instance holds the lease; this one is passive");
                    passive_logged = Some(Instant::now());
                }
            }
            Err(error) => {
                warn!(event = "lease.store_unavailable", lease = %name, code = error.error_code(), %error, "lease not renewed; the local deadline still stands");
            }
        }
    }
    if let Some(previous) = held.send_replace(None) {
        match lease.release().await {
            Ok(released) => {
                info!(event = "lease.released", lease = %name, epoch = previous.epoch(), ?released, "lease released on stop");
                if let Some(observer) = &observer {
                    observer.lost(name, previous.epoch(), LossReason::Released);
                }
            }
            Err(error) => warn!(
                event = "lease.release_failed",
                lease = %name,
                code = error.error_code(),
                %error,
                "lease not released; it expires by its TTL"
            ),
        }
    }
    if !*stop.borrow() {
        error!(event = "lease.surface_ended", lease = %name, "the lease's renewal stopped without being asked to");
    }
    ended.send_replace(true);
}

/// Forgets work keys past the retention window, measured on the database's clock.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn prune(lease: &Lease, proof: &HeldLease) {
    let window = lease.config().ttl() + lease.config().work_retention();
    let Some(cutoff) = TimeDelta::from_std(window)
        .ok()
        .and_then(|window| proof.expires_at().checked_sub_signed(window))
    else {
        warn!(event = "lease.prune_skipped", lease = %lease.name(), "the retention window leaves the timestamp range");
        return;
    };
    match timeout(lease.config().ttl(), proof.forget_work_before(cutoff)).await {
        Ok(Ok(0)) => debug!(lease = %lease.name(), %cutoff, "no work keys past retention"),
        Ok(Ok(forgotten)) => {
            info!(event = "lease.work_forgotten", lease = %lease.name(), forgotten, %cutoff, "work keys past retention forgotten")
        }
        Ok(Err(error)) => {
            warn!(event = "lease.prune_failed", lease = %lease.name(), code = error.error_code(), %error, "work keys not pruned; tried again next hour")
        }
        Err(_) => {
            warn!(event = "lease.prune_failed", lease = %lease.name(), "pruning work keys took longer than the TTL; abandoned")
        }
    }
}

/// Runs one work on its cadence while the lease is held.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn work_loop(
    lease: Lease,
    registration: Registration,
    held: watch::Receiver<Option<HeldLease>>,
    mut stop: watch::Receiver<bool>,
) {
    let name = lease.name();
    let mut ticker = interval(registration.every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
                continue;
            }
            _ = ticker.tick() => {}
        }
        let Some(proof) = held.borrow().clone() else {
            debug!(lease = %name, work = registration.name, "lease not held here; tick skipped");
            continue;
        };
        tokio::select! {
            biased;
            () = proof.lost() => warn!(
                event = "lease.tick_cancelled",
                lease = %name,
                work = registration.name,
                epoch = proof.epoch(),
                "the lease was lost mid-tick; the tick was dropped"
            ),
            outcome = registration.work.run(&proof) => match outcome {
                Ok(()) => debug!(lease = %name, work = registration.name, epoch = proof.epoch(), "leased work tick done"),
                Err(failure) => error!(
                    event = "lease.work_failed",
                    lease = %name,
                    work = registration.name,
                    epoch = proof.epoch(),
                    code = failure.get().error_code(),
                    error = %failure,
                    "leased work tick failed; it runs again next tick"
                ),
            },
        }
    }
}

/// An instant no renewal ever reaches, for a `select!` branch that is disabled anyway.
fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(86_400 * 365)
}
