//! A drain confined to one owner fires that owner's jobs and leaves every other job pending, and
//! a confined drain that cannot ask its buckets fails instead of reporting that nothing was due.

mod support;

use chrono::TimeDelta;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{JobConfinement, JobKind, ScheduleJobs, Scheduled};
use fundamental_macros::Variant;
use fundamental_runtime::OutboxDrain as _;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;
use support::{Deadline, Recording, database_now, drain, effects_for, job, subject};
use uuid::Uuid;

/// A second kind, served beside the deadlines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
enum Alert {
    #[variant(value = "hold.alert")]
    Late,
}

impl JobKind for Alert {}

async fn schedule_due(pool: &SqlPool, name: &str) {
    let due = database_now(pool).await - TimeDelta::seconds(1);
    let about = subject(name);
    let outcome = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.schedule(Deadline::HoldLapse, &about, due).await
        })
        .await
        .expect("scheduled");
    assert_eq!(outcome, Scheduled::Created);
}

#[tokio::test]
async fn a_confined_drain_fires_only_its_own_jobs() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let ours = Uuid::new_v4();
    let theirs = Uuid::new_v4();
    let mine = [
        format!("hold/{ours}/{}:1", Uuid::new_v4()),
        format!("invoice/{ours}"),
        format!("{}/3", ours.simple()),
    ];
    let other = format!("hold/{theirs}/{}:1", Uuid::new_v4());
    for name in mine.iter().chain([&other, &"canary/2026100912".to_owned()]) {
        schedule_due(&pool, name).await;
    }

    let confined =
        drain(&pool, Recording::new()).confined_to(JobConfinement::naming(ours).expect("an owner"));
    let report = confined.drain(100).await.expect("a drain tick");

    assert_eq!(report.claimed, 3, "{report:?}");
    for name in &mine {
        assert_eq!(effects_for(&pool, name).await, 1, "{name} fired");
    }
    for name in [other.as_str(), "canary/2026100912"] {
        assert_eq!(effects_for(&pool, name).await, 0, "{name} is not ours");
        let pending = job(&pool, Deadline::HoldLapse, &subject(name))
            .await
            .expect("still there");
        assert_eq!(pending.fired_at(), None);
        assert_eq!(pending.attempts(), 0, "never claimed");
    }
}

#[tokio::test]
async fn a_confined_drain_over_a_missing_table_fails() {
    let schema = SqlSchema::mint()
        .await
        .expect("a schema without the job table");
    let confined = drain(schema.pool(), Recording::new())
        .confined_to(JobConfinement::naming(Uuid::new_v4()).expect("an owner"));
    let refused = confined
        .drain(100)
        .await
        .expect_err("a tick that asked no bucket is not a tick where nothing was due");
    assert_eq!(refused.get().error_code(), "SQL_001", "{refused}");
}

#[tokio::test]
async fn a_confined_drain_stays_confined_when_it_serves_another_kind() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let ours = Uuid::new_v4();
    let mine = format!("hold/{ours}/{}:1", Uuid::new_v4());
    let other = format!("hold/{}/{}:1", Uuid::new_v4(), Uuid::new_v4());
    for name in [&mine, &other] {
        schedule_due(&pool, name).await;
    }

    let served = drain(&pool, Recording::new())
        .confined_to(JobConfinement::naming(ours).expect("an owner"))
        .serving::<Alert, _>(Recording::new())
        .expect("a second kind");
    let report = served.drain(100).await.expect("a drain tick");

    assert_eq!(report.claimed, 1, "{report:?}");
    assert_eq!(effects_for(&pool, &mine).await, 1);
    assert_eq!(effects_for(&pool, &other).await, 0, "not ours");
}
