//! Jobs that fell due while nothing was draining run once each after it starts, and a subject's
//! jobs run in due order.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::Variant as _;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{ScheduleJobs, Scheduled};
use fundamental_sql::SqlPool;
use support::{Deadline, Recording, database_now, drain, drain_until_idle, effects, subject};

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
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn jobs_that_fell_due_while_down_run_once_each_in_due_order() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let outage_began = database_now(&pool).await - TimeDelta::hours(6);
    // Written latest first and subjects out of due order, so neither writing order nor key passes
    // for due order.
    let mut expected = 0;
    for step in [3_i64, 0, 2, 1] {
        for (offset, kind) in Deadline::OPTIONS.iter().enumerate().rev() {
            for index in 0..12 {
                let minutes = step * 60 + i64::try_from(offset).expect("small") * 7 + index;
                schedule(
                    &pool,
                    *kind,
                    &format!("case/{index}/{step}"),
                    outage_began + TimeDelta::minutes(minutes),
                )
                .await;
                expected += 1;
            }
        }
    }

    drain_until_idle(&drain(&pool, Recording::new())).await;

    let effects = effects(&pool).await;
    assert_eq!(effects.len(), expected, "every job that fell due ran");
    let mut by_subject: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for effect in &effects {
        assert!(
            seen.insert((effect.kind.as_str(), effect.subject.as_str())),
            "{} {} ran twice",
            effect.kind,
            effect.subject
        );
        by_subject
            .entry(effect.subject.as_str())
            .or_default()
            .push(effect.due_at);
    }
    for (subject, dues) in by_subject {
        assert!(
            dues.windows(2).all(|pair| pair[0] <= pair[1]),
            "{subject} ran out of due order: {dues:?}"
        );
    }
}
