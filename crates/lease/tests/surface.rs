//! A leased surface on the runtime. Requires Docker.

mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use fundamental_core::AnyError;
use fundamental_db::{DatabaseError, IsolationLevel};
use fundamental_lease::{
    Acquisition, HeldLease, Lease, LeaseRow, LeaseState, LeasedSurface, LeasedWork,
};
use fundamental_runtime::{Surface, SurfaceGuard};
use support::{lease, quick, schema_with_leases};
use tokio::time::{Instant, sleep, timeout};
use tracing_test::traced_test;

struct Counting(Arc<AtomicU32>);

impl LeasedWork for Counting {
    fn run<'a>(
        &'a self,
        _lease: &'a HeldLease,
    ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

/// Starts, then never finishes; notes being dropped.
struct Endless {
    started: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

struct NoteDrop(Arc<AtomicBool>);

impl Drop for NoteDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl LeasedWork for Endless {
    fn run<'a>(
        &'a self,
        _lease: &'a HeldLease,
    ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + 'a>> {
        self.started.store(true, Ordering::SeqCst);
        let guard = NoteDrop(Arc::clone(&self.dropped));
        Box::pin(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}

async fn start(surface: LeasedSurface) -> Box<dyn SurfaceGuard> {
    Box::new(surface).start().await.expect("the surface starts")
}

/// Waits until `lease` holds its row, or fails after `bound`.
async fn until_held_by(lease: &Lease, bound: Duration) -> Duration {
    let began = Instant::now();
    loop {
        if let Ok((Some(LeaseState::Held { holder, .. }), _)) = lease.current().await
            && holder == lease.holder().as_uuid()
        {
            return began.elapsed();
        }
        assert!(began.elapsed() < bound, "not held within {bound:?}");
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn release_on_graceful_stop_hands_over_within_one_probe() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let guard = start(LeasedSurface::new(alpha.clone())).await;
    until_held_by(&alpha, Duration::from_secs(5)).await;
    guard.stop().await.expect("a clean stop");

    let beta = lease(schema.pool(), "keeper");
    let began = Instant::now();
    match beta.try_acquire().await.expect("an answer") {
        Acquisition::Held(proof) => assert!(proof.began()),
        Acquisition::HeldElsewhere { .. } => panic!("the stopped holder did not release"),
    }
    assert!(
        began.elapsed() < quick().renew_every(),
        "no TTL to wait out"
    );
}

#[tokio::test]
async fn a_dead_holder_is_replaced_within_ttl_plus_one_probe() {
    let schema = schema_with_leases().await;
    // A holder that takes the lease and then dies: it never renews and never releases.
    let dead = lease(schema.pool(), "keeper");
    match dead.try_acquire().await.expect("an answer") {
        Acquisition::Held(_) => {}
        Acquisition::HeldElsewhere { .. } => panic!("nobody else holds it yet"),
    }
    let died = Instant::now();
    let successor = lease(schema.pool(), "keeper");
    let guard = start(LeasedSurface::new(successor.clone())).await;
    until_held_by(&successor, Duration::from_secs(10)).await;
    let handover = died.elapsed();
    let bound = quick().ttl() + quick().renew_every() + Duration::from_millis(750);
    println!(
        "handover after a dead holder: {} ms (bound {} ms)",
        handover.as_millis(),
        bound.as_millis()
    );
    assert!(
        handover <= bound,
        "handover took {handover:?}, bound {bound:?}"
    );
    guard.stop().await.expect("a clean stop");
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_running_tick_is_cancelled_when_the_lease_is_lost() {
    let schema = schema_with_leases().await;
    let pool = schema.pool().clone();
    let alpha = lease(&pool, "keeper");
    let (started, dropped) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let guard = start(LeasedSurface::new(alpha.clone()).with_work(
        "endless",
        Arc::new(Endless {
            started: Arc::clone(&started),
            dropped: Arc::clone(&dropped),
        }),
        Duration::from_millis(100),
    ))
    .await;
    until_held_by(&alpha, Duration::from_secs(5)).await;
    let began = Instant::now();
    while !started.load(Ordering::SeqCst) {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the work never started"
        );
        sleep(Duration::from_millis(20)).await;
    }

    // Hold the lease row locked past the TTL, so no renewal can land.
    let locker = tokio::spawn({
        let pool = pool.clone();
        async move {
            pool.transaction(IsolationLevel::Serializable, async |tx| {
                tx.find_for_update::<LeaseRow>(&"keeper".to_owned()).await?;
                sleep(quick().ttl() + Duration::from_secs(1)).await;
                Ok::<_, DatabaseError>(())
            })
            .await
        }
    });
    let cancelled = timeout(quick().ttl() + Duration::from_secs(2), async {
        while !dropped.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(cancelled.is_ok(), "the tick outlived the lease");
    let _ = locker.await;
    guard.stop().await.expect("a clean stop");
}

#[tokio::test]
#[traced_test]
async fn a_second_task_in_the_holding_process_is_not_told_the_lease_is_elsewhere() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let (first, second) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let guard = start(
        LeasedSurface::new(alpha.clone())
            .with_work(
                "first",
                Arc::new(Counting(Arc::clone(&first))),
                Duration::from_millis(100),
            )
            .with_work(
                "second",
                Arc::new(Counting(Arc::clone(&second))),
                Duration::from_millis(100),
            ),
    )
    .await;
    until_held_by(&alpha, Duration::from_secs(5)).await;
    sleep(Duration::from_millis(1_500)).await;
    guard.stop().await.expect("a clean stop");
    assert!(first.load(Ordering::SeqCst) > 0 && second.load(Ordering::SeqCst) > 0);
    assert!(
        !logs_contain("lease.passive"),
        "the holding process called itself passive"
    );
}

#[tokio::test]
#[traced_test]
async fn a_passive_instance_logs_that_it_is_passive_not_that_it_failed() {
    let schema = schema_with_leases().await;
    let alpha = lease(schema.pool(), "keeper");
    let beta = lease(schema.pool(), "keeper");
    let holder = start(LeasedSurface::new(alpha.clone())).await;
    until_held_by(&alpha, Duration::from_secs(5)).await;
    let passive = start(LeasedSurface::new(beta)).await;
    sleep(quick().renew_every() * 3).await;
    passive.stop().await.expect("a clean stop");
    holder.stop().await.expect("a clean stop");
    assert!(logs_contain("lease.passive"));
    assert!(!logs_contain("lease.store_unavailable"));
    assert!(!logs_contain("lease.lost"), "the holder never lost it");
}

#[tokio::test]
async fn a_missing_table_fails_start_with_slease_009() {
    let schema = fundamental_test_containers::sql::SqlSchema::mint()
        .await
        .expect("a schema without the lease tables");
    let error = Box::new(LeasedSurface::new(lease(schema.pool(), "keeper")))
        .start()
        .await
        .err()
        .expect("start refuses");
    assert_eq!(error.get().error_code(), "SLEASE_009", "{error}");
}

/// Records every tenure begun and lost, in order.
#[derive(Default)]
struct Recorder(std::sync::Mutex<Vec<String>>);

impl fundamental_lease::LeaseObserver for Recorder {
    fn acquired(&self, _lease: fundamental_lease::LeaseName, epoch: i64, _gap: Option<Duration>) {
        if let Ok(mut events) = self.0.lock() {
            events.push(format!("acquired {epoch}"));
        }
    }

    fn lost(
        &self,
        _lease: fundamental_lease::LeaseName,
        epoch: i64,
        reason: fundamental_lease::LossReason,
    ) {
        if let Ok(mut events) = self.0.lock() {
            events.push(format!("lost {epoch} {reason:?}"));
        }
    }
}

#[tokio::test]
async fn a_resumed_tenure_is_announced_again() {
    let schema = schema_with_leases().await;
    let pool = schema.pool().clone();
    let config = fundamental_lease::LeaseConfig::new(fundamental_lease::LeaseSettings {
        ttl_secs: 4,
        renewals_per_ttl: 4,
        drain_grace_ms: 250,
        max_clock_skew_ms: 900,
        work_retention_hours: 1,
    })
    .expect("valid timings");
    let alpha = Lease::new(
        pool.clone(),
        fundamental_lease::LeaseName::new("keeper").expect("a name"),
        config,
    );
    let recorder = Arc::new(Recorder::default());
    let guard = start(LeasedSurface::new(alpha.clone()).with_observer(recorder.clone())).await;
    until_held_by(&alpha, Duration::from_secs(5)).await;

    // Block renewals past the local deadline (TTL less skew), then let them through.
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        tx.find_for_update::<LeaseRow>(&"keeper".to_owned()).await?;
        sleep(Duration::from_millis(3_500)).await;
        Ok::<_, DatabaseError>(())
    })
    .await
    .expect("the row was held");
    let began = Instant::now();
    loop {
        let events = recorder
            .0
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        if events
            .iter()
            .filter(|event| event.starts_with("acquired"))
            .count()
            >= 2
        {
            assert!(
                events.iter().any(|event| event.contains("Expired")),
                "{events:?}"
            );
            break;
        }
        assert!(
            began.elapsed() < Duration::from_secs(8),
            "never announced again: {events:?}"
        );
        sleep(Duration::from_millis(50)).await;
    }
    guard.stop().await.expect("a clean stop");
}
