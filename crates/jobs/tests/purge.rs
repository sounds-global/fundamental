//! A fired job's record is kept for the retention window and then purged; a pending job and a
//! recent record are never touched.

mod support;

use chrono::TimeDelta;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{JobsPurge, ScheduleJobs, Scheduled};
use support::{
    Deadline, Recording, count_jobs, database_now, drain, drain_until_idle, job, quick, subject,
};

#[tokio::test]
async fn a_purge_removes_only_fired_records_past_retention() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    for (name, due) in [
        ("hold/old", now - TimeDelta::seconds(2)),
        ("hold/recent", now - TimeDelta::seconds(1)),
        ("hold/pending", now + TimeDelta::days(3)),
    ] {
        let hold = subject(name);
        let outcome = pool
            .transaction(IsolationLevel::Serializable, async |tx| {
                tx.schedule(Deadline::HoldLapse, &hold, due).await
            })
            .await
            .expect("scheduled");
        assert_eq!(outcome, Scheduled::Created);
    }
    drain_until_idle(&drain(&pool, Recording::new())).await;
    pool.execute(
        "UPDATE fundamental_jobs SET due_at = now() - INTERVAL '9 days', \
         fired_at = now() - INTERVAL '8 days' WHERE subject = 'hold/old'",
    )
    .await
    .expect("age one record");

    let purged = JobsPurge::new(pool.clone(), quick())
        .expect("a purge")
        .purge()
        .await
        .expect("a purge");

    assert_eq!(purged, 1);
    assert_eq!(count_jobs(&pool).await, 2);
    assert!(
        job(&pool, Deadline::HoldLapse, &subject("hold/old"))
            .await
            .is_none()
    );
    assert!(
        job(&pool, Deadline::HoldLapse, &subject("hold/recent"))
            .await
            .is_some()
    );
    assert!(
        job(&pool, Deadline::HoldLapse, &subject("hold/pending"))
            .await
            .is_some()
    );
}
