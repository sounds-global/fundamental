//! A change that may end a job holds it first: a job falling due meanwhile waits for the
//! change's transaction, whose cancel wins, and the answer is judged by the database's clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::SqlDb;
use fundamental_db::{IsolationLevel, OutboxClaimer};
use fundamental_jobs::{
    Cancelled, FireOutcome, Held, JobBucket, JobError, JobRelay, JobScope, ScheduleJobs, Scheduled,
    ScheduledJob,
};
use fundamental_sql::SqlPool;
use support::{Deadline, Recording, database_now, effects, job, quick, subject};

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

async fn held(pool: &SqlPool, name: &str) -> Held {
    let hold = subject(name);
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        tx.hold(Deadline::HoldLapse, &hold).await
    })
    .await
    .expect("held")
}

#[tokio::test]
async fn a_hold_says_whether_the_job_is_pending_and_due_by_the_database_clock() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    schedule(&pool, "hold/later", now + TimeDelta::hours(1)).await;
    schedule(&pool, "hold/due", now - TimeDelta::seconds(1)).await;

    assert!(matches!(held(&pool, "hold/later").await, Held::Pending { due_at, at } if due_at > at));
    assert!(matches!(held(&pool, "hold/due").await, Held::Due { due_at, at } if due_at <= at));
    assert!(matches!(
        held(&pool, "hold/never").await,
        Held::NotPending { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_falling_due_inside_its_holder_waits_and_the_holder_s_cancel_wins() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    schedule(&pool, "hold/1", now - TimeDelta::seconds(1)).await;
    let scope = JobScope::of::<Deadline>().in_bucket(JobBucket::of(&subject("hold/1")));
    let claimed = OutboxClaimer::<ScheduledJob, SqlDb>::claim(
        &pool,
        &scope,
        10,
        now,
        now + TimeDelta::seconds(30),
    )
    .await
    .expect("a claim")
    .rows
    .remove(0);
    let relay = Arc::new(JobRelay::<Deadline, Recording>::new(
        pool.clone(),
        Recording::new(),
        &quick(),
    ));
    let hold = subject("hold/1");

    let (fire, cancelled) = pool
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let held = tx.hold(Deadline::HoldLapse, &hold).await?;
            assert!(matches!(held, Held::Due { .. }), "{held:?}");
            let (relay, claimed) = (Arc::clone(&relay), claimed.clone());
            let fire = tokio::spawn(async move { relay.fire(&claimed).await });
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert!(
                !fire.is_finished(),
                "the relay waits for the holder, but answered {:?}",
                fire.await
            );
            let cancelled = tx.cancel(Deadline::HoldLapse, &hold).await?;
            Ok::<_, JobError>((fire, cancelled))
        })
        .await
        .expect("held and cancelled");

    assert_eq!(cancelled, Cancelled::Removed, "the holder's cancel wins");
    let fired = fire.await.expect("the relay ran");
    assert_eq!(fired.expect("answered"), FireOutcome::Cancelled);
    assert!(effects(&pool).await.is_empty(), "the job never fired");
    assert!(job(&pool, Deadline::HoldLapse, &hold).await.is_none());
}
