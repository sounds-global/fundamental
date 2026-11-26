//! The job table's inventory reads a job waiting for its due time as a job at its schedule, and
//! a due job pushed further out, or of a kind this deploy does not know, as something to report.

mod support;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_db::{HeldCount, IsolationLevel};
use fundamental_jobs::{JobsDrain, ScheduleJobs, Scheduled};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::SqlPool;
use support::{
    Deadline, OldDeadline, Recording, database_now, drain, drain_until_idle, effects, quick,
    subject,
};
use tracing_test::traced_test;

async fn schedule(pool: &SqlPool, kind: Deadline, name: &str, due: DateTime<Utc>) {
    let subject = subject(name);
    let outcome = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.schedule(kind, &subject, due).await
        })
        .await
        .expect("scheduled");
    assert_eq!(outcome, Scheduled::Created);
}

#[tokio::test]
async fn the_inventory_is_clear_for_a_job_due_next_month() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let next_month = database_now(&pool).await + TimeDelta::days(30);
    schedule(&pool, Deadline::HoldLapse, "hold/1", next_month).await;

    let inventory = drain(&pool, Recording::new())
        .inventory()
        .await
        .expect("an inventory");

    assert!(inventory.is_clear(), "{inventory:?}");
}

#[tokio::test]
async fn a_due_job_pushed_out_by_hand_is_counted_beyond_policy() {
    let (_schema, pool) = support::schema_with_jobs().await;
    schedule(
        &pool,
        Deadline::HoldLapse,
        "hold/1",
        database_now(&pool).await - TimeDelta::hours(1),
    )
    .await;
    pool.execute("UPDATE fundamental_jobs SET next_attempt_at = now() + INTERVAL '30 days'")
        .await
        .expect("push the job out");

    let inventory = drain(&pool, Recording::new())
        .inventory()
        .await
        .expect("an inventory");

    assert_eq!(inventory.beyond_policy(), HeldCount::Exact(1));
}

#[tokio::test]
#[traced_test]
async fn a_kind_this_deploy_does_not_know_is_never_claimed_and_is_reported() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    schedule(&pool, Deadline::Reminder, "invoice/1", due).await;
    let older_deploy: JobsDrain<OldDeadline, Recording> =
        JobsDrain::new(pool.clone(), Recording::new(), quick()).expect("a drain");

    drain_until_idle(&older_deploy).await;
    older_deploy.inventory().await.expect("an inventory");

    assert!(
        effects(&pool).await.is_empty(),
        "the older deploy never claims it"
    );
    assert!(logs_contain("kinds this deploy does not declare"));

    drain_until_idle(&drain(&pool, Recording::new())).await;
    assert_eq!(
        effects(&pool).await.len(),
        1,
        "a deploy that knows it fires it"
    );
}
