//! A `JobsSurface` refuses a bad declaration before it starts, and once started fires what falls
//! due and stops cleanly.

mod support;

use std::time::Duration;

use chrono::TimeDelta;
use fundamental_core::Error as _;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{JobKind, JobsSettings, JobsSurface, ScheduleJobs, Scheduled};
use fundamental_macros::Variant;
use fundamental_runtime::Surface;
use support::{Deadline, Recording, database_now, effects, subject};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
enum Twice {
    #[variant(value = "same")]
    First,
    #[variant(value = "same")]
    Second,
}

impl JobKind for Twice {}

#[tokio::test]
async fn a_surface_refuses_duplicate_kinds_and_bad_settings() {
    let (_schema, pool) = support::schema_with_jobs().await;

    let kinds = JobsSurface::<Twice, Recording>::new(
        "jobs",
        pool.clone(),
        Recording::new(),
        JobsSettings::default(),
    )
    .err()
    .expect("refused");
    let settings = JobsSurface::<Deadline, Recording>::new(
        "jobs",
        pool,
        Recording::new(),
        JobsSettings {
            lease_secs: 0,
            ..JobsSettings::default()
        },
    )
    .err()
    .expect("refused");

    assert_eq!(kinds.error_code(), "JOBS_002");
    assert_eq!(settings.error_code(), "JOBS_010");
}

#[tokio::test]
async fn a_started_surface_fires_what_falls_due_and_stops() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let surface = JobsSurface::<Deadline, Recording>::new(
        "jobs",
        pool.clone(),
        Recording::new(),
        JobsSettings {
            poll_interval_ms: 100,
            ..support::quick()
        },
    )
    .expect("a surface");
    let guard = Box::new(surface).start().await.expect("started");

    let hold = subject("hold/1");
    let due = database_now(&pool).await + TimeDelta::milliseconds(300);
    let outcome = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.schedule(Deadline::HoldLapse, &hold, due).await
        })
        .await
        .expect("scheduled");
    assert_eq!(outcome, Scheduled::Created);

    let mut fired = Vec::new();
    for _ in 0..50 {
        fired = effects(&pool).await;
        if !fired.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    guard.stop().await.expect("a clean stop");

    assert_eq!(fired.len(), 1);
    assert!(fired[0].fired_at >= fired[0].due_at);
}
