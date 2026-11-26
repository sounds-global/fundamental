//! [`SyncClient`] — a self-healing device sync session for one slice binding.

use std::convert::Infallible;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use fundamental_offline::db::OfflineDatabase;
use fundamental_sync::adapters::OfflineMirror;
use fundamental_sync::wire::SliceChange;
use fundamental_sync::{
    LocalMirror, SliceBinding, SyncEngine, SyncRecord, Syncable, ensure_mirror_table,
};
use futures::future::{AbortHandle, Either, abortable, select};
use futures::{StreamExt, pin_mut};
use tracing::{debug, error, info, instrument, warn};
use web_time::Instant;

use fundamental_core::retry::Entropy;

use crate::reconnect_hint::ReconnectHint;
use crate::reconnect_schedule::ReconnectSchedule;
use crate::status_board::StatusBoard;
use crate::sync_client_config::SyncClientConfig;
use crate::{ConnectionStatus, ConnectionWatch, WaitCause, WsRemoteSource};

/// A device sync session that keeps one [`SliceBinding`] live in the offline database,
/// over one connection it owns, reconnecting and reconciling on its own.
///
/// It never gives up: only [`shutdown`](Self::shutdown) or dropping the handle ends it.
/// It runs as a local task, so on native it must start within a `tokio::task::LocalSet`.
#[derive(Debug)]
pub struct SyncClient {
    abort: AbortHandle,
    status: StatusBoard,
}

impl SyncClient {
    /// Starts a background session syncing `binding` into `db`, until the handle is dropped.
    ///
    /// # Panics
    /// On native targets, panics if called outside a `tokio::task::LocalSet`.
    #[must_use]
    pub fn start<M>(
        config: SyncClientConfig,
        binding: SliceBinding,
        db: Rc<OfflineDatabase>,
    ) -> Self
    where
        M: Syncable + 'static,
    {
        let status = StatusBoard::new();
        let (task, abort) = abortable(run_loop::<M>(config, binding, db, status.clone()));
        #[cfg(not(target_arch = "wasm32"))]
        tokio::task::spawn_local(async move {
            let _ = task.await;
        });
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let _ = task.await;
        });
        Self { abort, status }
    }

    /// The session's status now and every change after.
    #[must_use]
    pub fn status(&self) -> ConnectionWatch {
        self.status.watch()
    }

    /// Ends the session and publishes [`ConnectionStatus::Stopped`]; dropping the handle does the same.
    pub fn shutdown(self) {}
}

impl Drop for SyncClient {
    fn drop(&mut self) {
        info!("sync client shut down by its owner");
        self.status.publish(ConnectionStatus::Stopped);
        self.abort.abort();
    }
}

async fn sleep(dur: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(dur).await;
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::sleep(dur).await;
}

/// How a connection attempt ended, and the class of failure the next wait is for.
enum SessionEnd {
    /// The session was authenticated and then lost after `lived` in the live state.
    LostAfterConnected {
        lived: Duration,
        hint: Option<ReconnectHint>,
        cause: WaitCause,
    },
    /// The connection could not be established, authenticated or made live.
    FailedToConnect {
        hint: Option<ReconnectHint>,
        cause: WaitCause,
    },
}

/// The reconnect loop: run a session, then wait a full-jitter draw, for as long as the client lives.
#[instrument(skip(config, binding, db, status), fields(url = %config.url, slice = %binding.slice()))]
async fn run_loop<M>(
    config: SyncClientConfig,
    binding: SliceBinding,
    db: Rc<OfflineDatabase>,
    status: StatusBoard,
) where
    M: Syncable + 'static,
{
    let mut schedule = ReconnectSchedule::new();
    let _never: Infallible = drive(&mut schedule, &status, sleep, || {
        run_session::<M>(&config, &binding, &db, &status)
    })
    .await;
}

/// Runs `session` again and again; it ends only when its task is aborted.
async fn drive<E, S, SF, P, PF>(
    schedule: &mut ReconnectSchedule<E>,
    status: &StatusBoard,
    mut pause: P,
    mut session: S,
) -> Infallible
where
    E: Entropy,
    S: FnMut() -> SF,
    SF: Future<Output = SessionEnd>,
    P: FnMut(Duration) -> PF,
    PF: Future<Output = ()>,
{
    loop {
        status.publish(ConnectionStatus::Connecting);
        let end = session().await;
        let (delay, cause) = next_wait(schedule, end);
        status.publish(ConnectionStatus::Waiting {
            until: Instant::now() + delay,
            cause,
        });
        pause(delay).await;
    }
}

/// The wait before the next session; every ending draws from the schedule, none redials at once.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn next_wait<E: Entropy>(
    schedule: &mut ReconnectSchedule<E>,
    end: SessionEnd,
) -> (Duration, WaitCause) {
    match end {
        SessionEnd::LostAfterConnected { lived, hint, cause } => {
            log_hint(hint);
            let (delay, reset) = schedule.after_lost_session(lived, hint);
            info!(
                lived_ms = lived.as_millis(),
                reset,
                ?cause,
                delay_ms = delay.as_millis(),
                "sync connection lost; reconnecting after a jittered wait"
            );
            (delay, cause)
        }
        SessionEnd::FailedToConnect { hint, cause } => {
            log_hint(hint);
            let delay = schedule.after_failed_connect(hint);
            debug!(
                step = schedule.step(),
                ?cause,
                delay_ms = delay.as_millis(),
                "reconnect backoff"
            );
            (delay, cause)
        }
    }
}

/// Records a server's hint once, where it shapes the schedule.
fn log_hint(hint: Option<ReconnectHint>) {
    if let Some(hint) = hint {
        info!(
            ?hint,
            floor_ms = hint.floor().as_millis(),
            "server hint raised the reconnect floor"
        );
    }
}

/// One connection's lifecycle, until the connection drops.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn run_session<M>(
    config: &SyncClientConfig,
    binding: &SliceBinding,
    db: &OfflineDatabase,
    status: &StatusBoard,
) -> SessionEnd
where
    M: Syncable + 'static,
{
    // Idempotent, so a failure is retried with the session rather than ending the binding.
    if let Err(error) = ensure_mirror_table::<M>(db) {
        error!(%error, "could not create the offline mirror table");
        return SessionEnd::FailedToConnect {
            hint: None,
            cause: WaitCause::Local,
        };
    }
    let mut remote = match WsRemoteSource::connect_hinted(&config.url).await {
        Ok(remote) => remote,
        Err(failure) => {
            debug!(error = %failure.error, hint = ?failure.hint, "sync connect failed");
            return SessionEnd::FailedToConnect {
                hint: failure.hint,
                cause: WaitCause::of(&failure.error, failure.hint),
            };
        }
    };
    remote.set_request_timeout(config.request_timeout);
    let authenticated = match &config.auth {
        crate::SyncAuth::Jwt(token) => remote.authenticate(token).await,
        #[cfg(feature = "vault-auth")]
        crate::SyncAuth::Vault(owner) => remote.authenticate_vault(owner).await,
    };
    if let Err(error) = authenticated {
        debug!(%error, "sync authentication failed");
        let hint = remote.close_hint();
        return SessionEnd::FailedToConnect {
            hint,
            cause: WaitCause::of(&error, hint),
        };
    }
    let lost = |lived: Duration, cause: WaitCause| {
        let hint = remote.close_hint();
        SessionEnd::LostAfterConnected {
            lived,
            hint,
            cause: WaitCause::or_hinted(cause, hint),
        }
    };

    let mirror = match OfflineMirror::<M>::new(db) {
        Ok(mirror) => mirror,
        Err(error) => {
            error!(%error, "offline mirror unavailable");
            return SessionEnd::FailedToConnect {
                hint: None,
                cause: WaitCause::Local,
            };
        }
    };

    if let Err(error) = reconcile::<M>(&mirror, &remote, binding).await {
        warn!(%error, "initial reconcile pull failed");
    }

    if let Err(error) = push::<M>(&mirror, &remote, binding).await {
        warn!(%error, "initial dirty-row push failed");
    }

    // A poll-only session (a hub without `slice.subscribe`) converges on the reconcile timer;
    // the stream is still taken below, so a dropped socket is detected either way.
    if config.live_subscribe
        && let Err(error) = remote
            .subscribe_slice(binding.slice(), binding.params().clone())
            .await
    {
        // The session never went live, so it waits like a failed connect.
        warn!(%error, "subscribe failed");
        let hint = remote.close_hint();
        let cause = if remote.is_lost() {
            WaitCause::Closed
        } else {
            WaitCause::Rejected
        };
        return SessionEnd::FailedToConnect {
            hint,
            cause: WaitCause::or_hinted(cause, hint),
        };
    }

    let Some(mut stream) = remote.live_changes() else {
        error!("live-change stream already taken");
        return SessionEnd::FailedToConnect {
            hint: None,
            cause: WaitCause::Local,
        };
    };
    let live_since = Instant::now();
    let probe_every = config.probe_interval();
    info!(
        slice = binding.slice(),
        live_subscribe = config.live_subscribe,
        probe_ms = probe_every.as_millis(),
        "sync session live"
    );
    status.publish(ConnectionStatus::Live);

    loop {
        let next = stream.next();
        let timer = sleep(probe_every);
        pin_mut!(next);
        pin_mut!(timer);
        match select(next, timer).await {
            Either::Left((Some(change), _)) => apply::<M>(&mirror, change).await,
            Either::Left((None, _)) => return lost(live_since.elapsed(), WaitCause::Closed),
            Either::Right(((), _)) => {
                // The re-pull doubles as the liveness probe: a half-open socket never ends the live stream.
                debug!("periodic reconcile");
                if let Err(error) = reconcile::<M>(&mirror, &remote, binding).await {
                    if remote.is_lost() {
                        warn!(%error, "reconcile pull got no answer; ending the session");
                        return lost(live_since.elapsed(), WaitCause::TimedOut);
                    }
                    // The hub answered with an error: reconnecting would not change it.
                    warn!(%error, "reconcile pull failed; keeping the session");
                }
                if let Err(error) = push::<M>(&mirror, &remote, binding).await {
                    warn!(%error, "reconcile push failed");
                }
            }
        }
    }
}

async fn reconcile<M>(
    mirror: &OfflineMirror<'_, M>,
    remote: &WsRemoteSource,
    binding: &SliceBinding,
) -> Result<(), fundamental_sync::SyncError>
where
    M: Syncable,
{
    SyncEngine.pull(mirror, remote, binding).await.map(|_| ())
}

/// Pushes locally-dirty rows to the remote (idempotent).
async fn push<M>(
    mirror: &OfflineMirror<'_, M>,
    remote: &WsRemoteSource,
    binding: &SliceBinding,
) -> Result<(), fundamental_sync::SyncError>
where
    M: Syncable,
{
    SyncEngine.push(mirror, remote, binding).await.map(|_| ())
}

/// Applies one live change to the mirror; a malformed change is logged and dropped.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn apply<M>(mirror: &OfflineMirror<'_, M>, change: SliceChange)
where
    M: Syncable,
{
    match SyncRecord::try_from(change.record) {
        Ok(record) => {
            if let Err(error) = mirror.apply_remote(M::MIRROR_TABLE, &record).await {
                warn!(%error, "applying live change failed");
            }
        }
        Err(error) => warn!(%error, "malformed live change dropped"),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use fundamental_core::retry::SeededEntropy;
    use futures::FutureExt;

    use super::*;
    use crate::reconnect_schedule::STABLE_SESSION;

    fn lost(lived: Duration) -> SessionEnd {
        SessionEnd::LostAfterConnected {
            lived,
            hint: None,
            cause: WaitCause::Closed,
        }
    }

    fn failed(cause: WaitCause) -> SessionEnd {
        SessionEnd::FailedToConnect { hint: None, cause }
    }

    #[test]
    fn a_session_lost_before_it_was_stable_widens_the_next_wait() {
        let mut schedule = ReconnectSchedule::with_entropy(SeededEntropy::new(1));
        for _ in 0..4 {
            next_wait(&mut schedule, lost(STABLE_SESSION / 2));
        }
        assert_eq!(schedule.window(), Duration::from_secs(10));
    }

    #[test]
    fn the_wait_carries_the_class_of_its_failure() {
        let mut schedule = ReconnectSchedule::with_entropy(SeededEntropy::new(1));
        assert_eq!(
            next_wait(&mut schedule, failed(WaitCause::Refused)).1,
            WaitCause::Refused
        );
        assert_eq!(
            next_wait(&mut schedule, failed(WaitCause::Local)).1,
            WaitCause::Local
        );
        assert_eq!(
            next_wait(&mut schedule, lost(Duration::ZERO)).1,
            WaitCause::Closed
        );
    }

    /// Returns `Pending` once, as a real session's first network await does.
    async fn yield_once() {
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                std::task::Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        })
        .await;
    }

    /// How one scripted session ends.
    #[derive(Clone, Copy)]
    enum Scripted {
        /// Fails to connect, as the test's `fail` future says.
        Fails,
        /// Goes live, then its socket closes.
        LiveThenClosed,
    }

    /// Drives the real loop with scripted sessions, returning every status seen up to the final
    /// `Live` and how many sessions started.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn script<F, FF>(plan: &[Scripted], fail: F) -> (Vec<ConnectionStatus>, usize)
    where
        F: Fn() -> FF,
        FF: Future<Output = SessionEnd>,
    {
        let status = StatusBoard::new();
        let mut watch = status.watch();
        let attempts = Cell::new(0usize);
        let seen = RefCell::new(vec![watch.current()]);
        let mut schedule = ReconnectSchedule::with_entropy(SeededEntropy::new(7));
        let lives_before_the_last = plan
            .iter()
            .filter(|step| matches!(step, Scripted::LiveThenClosed))
            .count();
        let observer = async {
            let mut lives = 0;
            while let Some(next) = watch.changed().await {
                seen.borrow_mut().push(next);
                if next == ConnectionStatus::Live {
                    lives += 1;
                    if lives > lives_before_the_last {
                        break;
                    }
                }
            }
        };
        let looped = drive(&mut schedule, &status, sleep, || {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            let status = status.clone();
            let step = plan.get(attempt);
            let failing = matches!(step, Some(Scripted::Fails)).then(&fail);
            let closes = matches!(step, Some(Scripted::LiveThenClosed));
            async move {
                // Gives the observer its turn to see `Connecting`.
                yield_once().await;
                if let Some(failing) = failing {
                    return failing.await;
                }
                status.publish(ConnectionStatus::Live);
                if closes {
                    yield_once().await;
                    return lost(Duration::from_secs(60));
                }
                std::future::pending::<SessionEnd>().await
            }
        });
        {
            futures::pin_mut!(looped);
            futures::pin_mut!(observer);
            let _ = select(observer, looped.as_mut().map(|never| match never {})).await;
        }
        (seen.into_inner(), attempts.get())
    }

    const THROUGH_A_LOSS: [&str; 7] = [
        "connecting",
        "waiting:Refused",
        "connecting",
        "live",
        "waiting:Closed",
        "connecting",
        "live",
    ];

    fn shape(seen: &[ConnectionStatus]) -> Vec<String> {
        seen.iter()
            .map(|status| match status {
                ConnectionStatus::Connecting => "connecting".to_owned(),
                ConnectionStatus::Live => "live".to_owned(),
                ConnectionStatus::Waiting { cause, .. } => format!("waiting:{cause:?}"),
                ConnectionStatus::Stopped => "stopped".to_owned(),
            })
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(start_paused = true)]
    async fn no_count_of_failed_connects_stops_the_loop() {
        // Bounded, so a loop that stopped fails instead of hanging; the clock is paused.
        let bound = Duration::from_secs(10) * 10_001;
        let (seen, attempts) = tokio::time::timeout(
            bound,
            script(&vec![Scripted::Fails; 10_000], || async {
                failed(WaitCause::Refused)
            }),
        )
        .await
        .unwrap_or_else(|_| panic!("the loop stopped before the hub came back"));
        assert_eq!(
            attempts, 10_001,
            "every failure was followed by another attempt"
        );
        assert_eq!(seen.last(), Some(&ConnectionStatus::Live));
        assert!(
            !seen.contains(&ConnectionStatus::Stopped),
            "only the app stops a session"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(start_paused = true)]
    async fn the_status_moves_through_a_failure_and_a_lost_session() {
        let plan = [Scripted::Fails, Scripted::LiveThenClosed];
        let (seen, attempts) = script(&plan, || async { failed(WaitCause::Refused) }).await;
        assert_eq!(attempts, 3);
        assert_eq!(shape(&seen), THROUGH_A_LOSS);
    }

    /// The wasm build, under the runner's WHATWG `WebSocket`.
    #[cfg(target_arch = "wasm32")]
    mod browser {
        use wasm_bindgen_test::wasm_bindgen_test;

        use super::*;

        /// Nothing listens on the discard port, so the `WebSocket` fails for real.
        async fn refused_connect() -> SessionEnd {
            match WsRemoteSource::connect_hinted("ws://127.0.0.1:9").await {
                Ok(_) => panic!("something answered on the discard port"),
                Err(failure) => SessionEnd::FailedToConnect {
                    hint: failure.hint,
                    cause: WaitCause::of(&failure.error, failure.hint),
                },
            }
        }

        #[wasm_bindgen_test]
        async fn the_status_moves_through_a_refused_socket_and_a_lost_session_in_the_browser() {
            let plan = [Scripted::Fails, Scripted::LiveThenClosed];
            let (seen, attempts) = script(&plan, refused_connect).await;
            assert_eq!(attempts, 3);
            assert_eq!(shape(&seen), THROUGH_A_LOSS);
        }

        #[wasm_bindgen_test]
        async fn repeated_failures_keep_the_loop_going_in_the_browser() {
            let plan = [Scripted::Fails, Scripted::Fails, Scripted::Fails];
            let (seen, attempts) = script(&plan, || async { failed(WaitCause::TimedOut) }).await;
            assert_eq!(attempts, 4);
            assert_eq!(seen.last(), Some(&ConnectionStatus::Live));
            assert!(!seen.contains(&ConnectionStatus::Stopped));
        }
    }
}
