//! A handler's verdict: a job that postponed itself fires at its new due time and never before, a
//! deferred one keeps its due time and counts no failure, a re-arm replaces a postpone, and only
//! the worker holding the claim writes a verdict at all.

mod support;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::SqlDb;
use fundamental_db::{IsolationLevel, OutboxClaimer};
use fundamental_jobs::{
    FireOutcome, JobBucket, JobHandler, JobRelay, JobScope, JobsDrain, JobsSettings, ScheduleJobs,
    Scheduled, ScheduledJob,
};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::SqlPool;
use support::{
    Deadline, Effect, OldDeadline, Scripted, Step, database_now, effects, job, quick, subject,
};
use tracing_test::traced_test;

/// One-second leases and backoffs, a two-second defer — longer than the longest backoff, so only
/// the declared recurrence keeps a deferred job inside the policy — and a one-minute overdue mark.
fn settings() -> JobsSettings {
    JobsSettings {
        defer_secs: 2,
        overdue_warn_secs: 60,
        ..quick()
    }
}

fn drain<H: JobHandler<Deadline>>(pool: &SqlPool, handler: Arc<H>) -> JobsDrain<Deadline, H> {
    JobsDrain::new(pool.clone(), handler, settings()).expect("a drain")
}

async fn schedule(pool: &SqlPool, kind: Deadline, name: &str, due: DateTime<Utc>) -> Scheduled {
    let subject = subject(name);
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        tx.schedule(kind, &subject, due).await
    })
    .await
    .expect("scheduled")
}

async fn create(pool: &SqlPool, kind: Deadline, name: &str, due: DateTime<Utc>) {
    assert_eq!(schedule(pool, kind, name, due).await, Scheduled::Created);
}

async fn hold(pool: &SqlPool) -> ScheduledJob {
    job(pool, Deadline::HoldLapse, &subject("hold/1"))
        .await
        .expect("the job")
}

/// Ticks `drain` until the handler has been asked `attempts` times, for at most ten seconds.
async fn until_asked(pool: &SqlPool, drain: &dyn OutboxDrain, attempts: usize) -> Vec<Effect> {
    for _ in 0..100 {
        drain.drain(100).await.expect("a drain tick");
        let effects = effects(pool).await;
        if effects.len() >= attempts {
            return effects;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the handler was not asked {attempts} times");
}

#[tokio::test]
async fn a_postponed_job_never_fires_before_its_new_due() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(
        &pool,
        Scripted::new([Step::PostponeBy(TimeDelta::seconds(3))]),
    );

    let report = drain.drain(100).await.expect("a drain tick");
    assert_eq!(report.claimed, 1);
    let postponing = effects(&pool).await;
    assert_eq!(
        postponing.len(),
        1,
        "the postponing attempt's writes committed with its verdict"
    );
    let postponed = hold(&pool).await;
    let new_due = postponing[0].fired_at + TimeDelta::seconds(3);
    assert_eq!(postponed.fired_at(), None, "a postponed job is pending");
    assert_eq!(postponed.due_at(), new_due);
    assert_eq!(postponed.next_attempt_at(), new_due);
    assert_eq!(postponed.attempts(), 0);
    assert_eq!(postponed.last_error(), None);

    let effects = until_asked(&pool, &drain, 2).await;
    assert!(
        effects[1].fired_at >= new_due,
        "asked again at {} before its new due {new_due}",
        effects[1].fired_at
    );
    assert_eq!(effects[1].due_at, new_due, "the handler sees its new due");
    let fired = hold(&pool).await;
    assert!(fired.fired_at().is_some_and(|at| at >= new_due));
}

#[tokio::test]
async fn a_postpone_to_the_past_is_a_handler_failure() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(
        &pool,
        Scripted::new([Step::PostponeFromDue(-TimeDelta::seconds(10))]),
    );

    drain.drain(100).await.expect("a drain tick");

    assert!(
        effects(&pool).await.is_empty(),
        "the refused attempt's writes rolled back"
    );
    let refused = hold(&pool).await;
    assert_eq!(refused.fired_at(), None);
    assert_eq!(refused.due_at(), due, "the due time is untouched");
    assert_eq!(refused.attempts(), 1, "counted as a failure");
    assert!(
        refused
            .last_error()
            .is_some_and(|error| error.starts_with("[JOBS_013]")),
        "{:?}",
        refused.last_error()
    );
}

#[tokio::test]
async fn a_deferred_job_keeps_its_due_and_counts_no_failure() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(&pool, Scripted::new([Step::Defer]));

    drain.drain(100).await.expect("a drain tick");
    let after = database_now(&pool).await;

    let deferring = effects(&pool).await;
    assert_eq!(
        deferring.len(),
        1,
        "the deferring attempt's writes committed"
    );
    let deferred = hold(&pool).await;
    assert_eq!(deferred.fired_at(), None);
    assert_eq!(deferred.due_at(), due, "a deferral keeps the due time");
    assert_eq!(deferred.attempts(), 0, "and counts no failure");
    assert_eq!(deferred.last_error(), None);
    let defer = TimeDelta::seconds(2);
    assert!(
        deferred.next_attempt_at() > deferring[0].fired_at + defer * 3 / 4,
        "at least three quarters of the cadence: {}",
        deferred.next_attempt_at()
    );
    assert!(
        deferred.next_attempt_at() <= after + defer,
        "at most one cadence after the handler returned: {}",
        deferred.next_attempt_at()
    );
    let inventory = drain.inventory().await.expect("an inventory");
    assert!(
        inventory.is_clear(),
        "a deferral is inside the policy: {inventory:?}"
    );

    let effects = until_asked(&pool, &drain, 2).await;
    assert!(effects[1].fired_at >= deferred.next_attempt_at());
    assert_eq!(effects[1].due_at, due);
    assert!(hold(&pool).await.fired_at().is_some());
}

#[tokio::test]
async fn a_slow_deferring_handler_is_asked_again_a_cadence_after_it_answered() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let held = TimeDelta::milliseconds(2_500);
    let drain = drain(
        &pool,
        Scripted::new([Step::DeferAfter(held.to_std().expect("a span"))]),
    );

    drain.drain(100).await.expect("a drain tick");

    let deferring = effects(&pool).await;
    let defer = TimeDelta::seconds(2);
    let next = hold(&pool).await.next_attempt_at();
    assert!(
        next >= deferring[0].fired_at + held + defer * 3 / 4,
        "measured from when the handler answered, not from its mark: {next}"
    );
}

#[tokio::test]
async fn a_worker_that_outlived_its_lease_cannot_undo_a_deferral() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    let due = now - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let scope = JobScope::of::<Deadline>().in_bucket(JobBucket::of(&subject("hold/1")));
    let stale = OutboxClaimer::<ScheduledJob, SqlDb>::claim(
        &pool,
        &scope,
        10,
        now,
        now + TimeDelta::seconds(1),
    )
    .await
    .expect("a claim")
    .rows
    .into_iter()
    .next()
    .expect("the job is claimed");
    tokio::time::sleep(Duration::from_millis(1_300)).await;
    drain(&pool, Scripted::new([Step::Defer]))
        .drain(100)
        .await
        .expect("the holder's tick");
    let deferred = hold(&pool).await;

    let answered =
        JobRelay::<Deadline, Scripted>::new(pool.clone(), Scripted::new([Step::Done]), &settings())
            .fire(&stale)
            .await
            .expect("answered");

    assert_eq!(answered, FireOutcome::Superseded);
    assert_eq!(
        effects(&pool).await.len(),
        1,
        "only the holder's handler ran"
    );
    assert_eq!(hold(&pool).await, deferred, "the deferral stands");
}

#[tokio::test]
async fn a_deferral_far_past_the_longest_backoff_is_inside_the_policy() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let ten_minutes = JobsSettings {
        defer_secs: 600,
        ..settings()
    };
    let drain: JobsDrain<Deadline, Scripted> =
        JobsDrain::new(pool.clone(), Scripted::new([Step::Defer]), ten_minutes).expect("a drain");

    drain.drain(100).await.expect("a drain tick");

    assert!(
        hold(&pool).await.next_attempt_at() > database_now(&pool).await + TimeDelta::minutes(7),
        "deferred minutes past the one-second backoff"
    );
    let inventory = drain.inventory().await.expect("an inventory");
    assert!(
        inventory.is_clear(),
        "the defer cadence is the policy's recurrence: {inventory:?}"
    );
}

#[tokio::test]
async fn a_rearmed_job_asks_its_handler_again() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(
        &pool,
        Scripted::new([Step::PostponeBy(TimeDelta::hours(1))]),
    );
    drain.drain(100).await.expect("a drain tick");
    let postponed_to = hold(&pool).await.due_at();

    let rearmed = schedule(&pool, Deadline::HoldLapse, "hold/1", due).await;

    assert_eq!(
        rearmed,
        Scheduled::Moved { from: postponed_to },
        "the last writer of the due time wins"
    );
    let effects = until_asked(&pool, &drain, 2).await;
    assert_eq!(
        effects[1].due_at, due,
        "the handler is asked again, at its re-armed due time"
    );
    assert!(hold(&pool).await.fired_at().is_some());
}

#[tokio::test]
async fn a_stale_worker_cannot_write_a_verdict() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    let due = now - TimeDelta::seconds(1);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let scope = JobScope::of::<Deadline>().in_bucket(JobBucket::of(&subject("hold/1")));
    let claim = async |at: DateTime<Utc>, lease: TimeDelta| {
        OutboxClaimer::<ScheduledJob, SqlDb>::claim(&pool, &scope, 10, at, at + lease)
            .await
            .expect("a claim")
            .rows
            .into_iter()
            .next()
            .expect("the job is claimed")
    };
    let stale = claim(now, TimeDelta::seconds(1)).await;
    tokio::time::sleep(Duration::from_millis(1_300)).await;
    let holder = claim(database_now(&pool).await, TimeDelta::seconds(30)).await;

    let answered = JobRelay::<Deadline, Scripted>::new(
        pool.clone(),
        Scripted::new([Step::PostponeBy(TimeDelta::hours(1))]),
        &settings(),
    )
    .fire(&stale)
    .await
    .expect("answered");

    assert_eq!(answered, FireOutcome::Superseded);
    assert!(effects(&pool).await.is_empty(), "its handler never ran");
    let row = hold(&pool).await;
    assert_eq!(row.fired_at(), None);
    assert_eq!(row.due_at(), due, "no postpone was written");
    assert_eq!(
        row.next_attempt_at(),
        holder.next_attempt_at(),
        "the row is as the claim holder left it"
    );
}

#[tokio::test]
#[traced_test]
async fn the_drain_counts_each_verdict_apart() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    for name in ["hold/1", "hold/2", "hold/3"] {
        create(&pool, Deadline::HoldLapse, name, due).await;
    }
    let drain = drain(
        &pool,
        Scripted::new([
            Step::Done,
            Step::PostponeBy(TimeDelta::hours(1)),
            Step::Defer,
        ]),
    );

    let report = drain.drain(100).await.expect("a drain tick");

    assert_eq!(report.claimed, 3);
    assert_eq!(
        report.relayed, 1,
        "the report the surface logs and meters counts fires only"
    );
    assert!(logs_contain(
        "job drain tick answered claimed=3 fired=1 postponed=1 deferred=1"
    ));
}

#[tokio::test]
#[traced_test]
async fn the_inventory_warns_of_an_overdue_job() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::minutes(5);
    create(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(&pool, Scripted::new([Step::Defer]));
    drain.drain(100).await.expect("a drain tick");

    drain.inventory().await.expect("an inventory");

    assert!(logs_contain("jobs are pending past their due time"));
    assert!(logs_contain("overdue=1"));
}

#[tokio::test]
#[traced_test]
async fn an_overdue_job_of_another_deploys_kind_is_not_counted() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::minutes(5);
    create(&pool, Deadline::Reminder, "invoice/1", due).await;
    let older_deploy: JobsDrain<OldDeadline, Scripted> =
        JobsDrain::new(pool.clone(), Scripted::new([]), settings()).expect("a drain");

    older_deploy.inventory().await.expect("an inventory");

    assert!(logs_contain("kinds this deploy does not declare"));
    assert!(!logs_contain("jobs are pending past their due time"));
}
