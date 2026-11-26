//! One drain serves several kind sets — an application's and a library's — on one job table: each
//! kind fires through its own handler, nothing warns about a kind a served set declares, and no
//! kind value may be served twice.

mod support;

use chrono::TimeDelta;
use fundamental_core::Error as _;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{JobKind, JobsDrain, JobsSettings, JobsSurface, ScheduleJobs, Scheduled};
use fundamental_macros::Variant;
use fundamental_runtime::OutboxDrain;
use support::{Deadline, Recording, database_now, drain_until_idle, effects, quick, subject};
use tracing_test::traced_test;

/// A library's own deadlines, beside the application's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
enum LibraryDeadline {
    #[variant(value = "library.expire")]
    Expire,
}

impl JobKind for LibraryDeadline {}

/// A library that claims one of the application's kind values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
enum Clashing {
    #[variant(value = "hold.lapse")]
    HoldLapse,
}

impl JobKind for Clashing {}

#[tokio::test]
#[traced_test]
async fn two_kind_sets_on_one_table_raise_no_unknown_kind_warning() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    let (hold, expire) = (subject("hold/1"), subject("library/1"));
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        assert_eq!(
            tx.schedule(Deadline::HoldLapse, &hold, due).await?,
            Scheduled::Created
        );
        assert_eq!(
            tx.schedule(LibraryDeadline::Expire, &expire, due).await?,
            Scheduled::Created
        );
        Ok::<_, fundamental_jobs::JobError>(())
    })
    .await
    .expect("scheduled");

    let drain: JobsDrain<Deadline, Recording> =
        JobsDrain::new(pool.clone(), Recording::new(), quick())
            .expect("a drain")
            .serving::<LibraryDeadline, Recording>(Recording::new())
            .expect("a second set");
    drain_until_idle(&drain).await;
    drain.inventory().await.expect("an inventory");

    let mut fired: Vec<String> = effects(&pool).await.into_iter().map(|e| e.kind).collect();
    fired.sort();
    assert_eq!(
        fired,
        ["hold.lapse", "library.expire"],
        "each set fired its own kind once"
    );
    assert!(!logs_contain("kinds this deploy does not declare"));
}

#[tokio::test]
async fn a_surface_refuses_a_kind_two_sets_declare() {
    let (_schema, pool) = support::schema_with_jobs().await;

    let surface = JobsSurface::<Deadline, Recording>::new(
        "jobs",
        pool.clone(),
        Recording::new(),
        JobsSettings::default(),
    )
    .expect("a surface")
    .serving::<Clashing, Recording>(Recording::new())
    .err()
    .expect("refused");
    let drain = JobsDrain::<Deadline, Recording>::new(pool, Recording::new(), quick())
        .expect("a drain")
        .serving::<Clashing, Recording>(Recording::new())
        .err()
        .expect("refused");

    for error in [surface, drain] {
        assert_eq!(error.error_code(), "JOBS_002");
        assert!(format!("{error:?}").contains("served_twice"), "{error:?}");
    }
}
