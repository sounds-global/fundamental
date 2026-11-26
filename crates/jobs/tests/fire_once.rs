//! Every job's effect commits exactly once — with two replicas draining, a worker killed mid-job,
//! a worker that outlived its lease — and never before its due time by the database's clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::SqlDb;
use fundamental_db::{ConstraintKind, IsolationLevel, OutboxClaimer};
use fundamental_jobs::{
    FireOutcome, JobBucket, JobRelay, JobScope, ScheduleJobs, Scheduled, ScheduledJob,
};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::{SqlError, SqlPool};
use support::{
    Deadline, Parking, Recording, database_now, drain, drain_until_idle, effects, effects_for, job,
    quick, subject,
};
use tokio::sync::Notify;

async fn schedule(pool: &SqlPool, name: &str, due: DateTime<Utc>) {
    let hold = subject(name);
    let outcome = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.schedule(Deadline::HoldLapse, &hold, due).await
        })
        .await
        .expect("scheduled");
    assert_eq!(outcome, Scheduled::Created);
}

/// Claims `name`'s job as a worker would, leasing it until `lease` past `now`.
async fn claim(pool: &SqlPool, name: &str, now: DateTime<Utc>, lease: TimeDelta) -> ScheduledJob {
    let scope = JobScope::of::<Deadline>().in_bucket(JobBucket::of(&subject(name)));
    let claim = OutboxClaimer::<ScheduledJob, SqlDb>::claim(pool, &scope, 10, now, now + lease)
        .await
        .expect("a claim");
    claim
        .rows
        .into_iter()
        .find(|row| row.subject() == name)
        .expect("the job is claimed")
}

/// Waits past a one-second lease by the database's clock.
async fn past_the_lease() {
    tokio::time::sleep(Duration::from_millis(1_300)).await;
}

#[tokio::test]
async fn two_replicas_fire_each_job_once() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    for index in 0..120 {
        schedule(&pool, &format!("hold/{index}"), due).await;
    }
    let first = drain(&pool, Recording::new());
    let second = drain(&pool, Recording::new());

    tokio::join!(drain_until_idle(&first), drain_until_idle(&second));
    past_the_lease().await;
    tokio::join!(drain_until_idle(&first), drain_until_idle(&second));

    let effects = effects(&pool).await;
    assert_eq!(effects.len(), 120, "every job fired");
    for index in 0..120 {
        assert_eq!(
            effects_for(&pool, &format!("hold/{index}")).await,
            1,
            "hold/{index} fired exactly once"
        );
    }
}

/// Waits until the database's clock is past `until`.
/// By the database's clock, the one a lease is judged by.
async fn past(pool: &SqlPool, until: DateTime<Utc>) {
    while database_now(pool).await <= until {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_worker_killed_after_claim_leaves_the_job_to_another_replica() {
    let (_schema, pool) = support::schema_with_jobs().await;
    schedule(
        &pool,
        "hold/1",
        database_now(&pool).await - TimeDelta::seconds(1),
    )
    .await;

    // The worker holding the claim dies before relaying anything.
    let claimed_at = database_now(&pool).await;
    let lease = TimeDelta::seconds(10);
    let _abandoned = claim(&pool, "hold/1", claimed_at, lease).await;
    let other = drain(&pool, Recording::new());
    drain_until_idle(&other).await;
    assert!(
        database_now(&pool).await < claimed_at + lease,
        "the drain outlasted the lease, so this run proves nothing about it"
    );
    assert!(effects(&pool).await.is_empty(), "the lease keeps the job");

    past(&pool, claimed_at + lease).await;
    drain_until_idle(&other).await;

    assert_eq!(effects_for(&pool, "hold/1").await, 1);
}

#[tokio::test]
async fn a_worker_killed_inside_the_fire_transaction_fires_nothing_then_once() {
    let (_schema, pool) = support::schema_with_jobs().await;
    schedule(
        &pool,
        "hold/1",
        database_now(&pool).await - TimeDelta::seconds(1),
    )
    .await;
    let parking = Arc::new(Parking {
        reached: Notify::new(),
    });
    let doomed = drain(&pool, Recording::parking(Arc::clone(&parking)));

    // Killed before it commits: dropping the task drops its connection, as a dead process would.
    let worker = tokio::spawn(async move {
        let _never = doomed.drain(10).await;
    });
    parking.reached.notified().await;
    worker.abort();
    let _aborted = worker.await;

    let pending = job(&pool, Deadline::HoldLapse, &subject("hold/1"))
        .await
        .expect("the job is still there");
    assert_eq!(
        pending.fired_at(),
        None,
        "the killed fire committed nothing"
    );
    assert!(effects(&pool).await.is_empty());

    past_the_lease().await;
    drain_until_idle(&drain(&pool, Recording::new())).await;

    assert_eq!(effects_for(&pool, "hold/1").await, 1);
}

#[tokio::test]
async fn a_paused_worker_whose_lease_lapsed_never_fires_again() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    schedule(&pool, "hold/1", now - TimeDelta::seconds(1)).await;
    let paused = claim(&pool, "hold/1", now, TimeDelta::seconds(1)).await;

    past_the_lease().await;
    drain_until_idle(&drain(&pool, Recording::new())).await;
    let late = JobRelay::<Deadline, Recording>::new(pool.clone(), Recording::new(), &quick())
        .fire(&paused)
        .await
        .expect("answered");

    assert_eq!(late, FireOutcome::AlreadyFired);
    assert_eq!(effects_for(&pool, "hold/1").await, 1);
}

#[tokio::test]
async fn a_paused_worker_is_superseded_by_the_claim_that_took_its_job() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    schedule(&pool, "hold/1", now - TimeDelta::seconds(1)).await;
    let paused = claim(&pool, "hold/1", now, TimeDelta::seconds(1)).await;

    past_the_lease().await;
    let _reclaimed = claim(
        &pool,
        "hold/1",
        database_now(&pool).await,
        TimeDelta::seconds(30),
    )
    .await;
    let late = JobRelay::<Deadline, Recording>::new(pool.clone(), Recording::new(), &quick())
        .fire(&paused)
        .await
        .expect("answered");

    assert_eq!(late, FireOutcome::Superseded);
    assert!(effects(&pool).await.is_empty());
}

#[tokio::test]
async fn a_job_never_fires_before_its_due_time_by_the_database_clock() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    let due = now + TimeDelta::hours(1);
    schedule(&pool, "hold/1", due).await;

    // A replica whose clock runs two hours ahead claims the job early.
    let early = claim(
        &pool,
        "hold/1",
        now + TimeDelta::hours(2),
        TimeDelta::seconds(30),
    )
    .await;
    let outcome = JobRelay::<Deadline, Recording>::new(pool.clone(), Recording::new(), &quick())
        .fire(&early)
        .await
        .expect("answered");

    assert_eq!(outcome, FireOutcome::NotYetDue);
    assert!(effects(&pool).await.is_empty());
    let waiting = job(&pool, Deadline::HoldLapse, &subject("hold/1"))
        .await
        .expect("the job");
    assert_eq!(
        waiting.next_attempt_at(),
        waiting.due_at(),
        "back at its due time"
    );
    assert_eq!(waiting.attempts(), 0, "an early claim is not a failure");
}

#[tokio::test]
async fn the_table_refuses_a_fire_recorded_before_its_due() {
    let (_schema, pool) = support::schema_with_jobs().await;
    schedule(
        &pool,
        "hold/1",
        database_now(&pool).await + TimeDelta::hours(1),
    )
    .await;

    let refused = pool
        .execute("UPDATE fundamental_jobs SET fired_at = due_at - INTERVAL '1 second'")
        .await;

    let error = refused.expect_err("the CHECK refuses it");
    assert!(
        matches!(
            &error,
            SqlError::ConstraintViolation {
                kind: ConstraintKind::Check,
                constraint: Some(constraint),
                ..
            } if constraint == "fundamental_jobs_fired_not_before_due"
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_failing_handler_backs_off_and_fires_once_it_succeeds() {
    let (_schema, pool) = support::schema_with_jobs().await;
    schedule(
        &pool,
        "hold/1",
        database_now(&pool).await - TimeDelta::seconds(1),
    )
    .await;
    let flaky = drain(&pool, Recording::refusing(1));

    drain_until_idle(&flaky).await;
    let failed = job(&pool, Deadline::HoldLapse, &subject("hold/1"))
        .await
        .expect("the job");
    assert_eq!(failed.attempts(), 1);
    assert_eq!(failed.fired_at(), None);
    let recorded = failed.last_error().expect("the failure is recorded");
    assert!(
        recorded.contains("JOBS_011") && recorded.contains("TEST_JOB_001"),
        "recorded by code: {recorded}"
    );
    assert!(
        effects(&pool).await.is_empty(),
        "the refused fire rolled back"
    );

    past_the_lease().await;
    drain_until_idle(&flaky).await;

    assert_eq!(effects_for(&pool, "hold/1").await, 1);
}
