//! A job is one row per (kind, subject), written in the caller's transaction: scheduling it
//! twice leaves one row, a reschedule and a cancel are one statement each, and an occurrence that
//! fired is reported rather than scheduled again.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, SubsecRound as _, TimeDelta, Utc};
use fundamental_core::Error as _;
use fundamental_db::IsolationLevel;
use fundamental_jobs::{Cancelled, JobError, Rescheduled, ScheduleJobs, Scheduled};
use fundamental_sql::SqlPool;
use support::{
    Deadline, Recording, count_jobs, database_now, drain, drain_until_idle, job, subject,
};
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing::instrument::WithSubscriber as _;
use tracing::span::{Attributes, Id};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::Registry;

async fn schedule(
    pool: &SqlPool,
    kind: Deadline,
    name: &str,
    due: DateTime<Utc>,
) -> Result<Scheduled, JobError> {
    let subject = subject(name);
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        tx.schedule(kind, &subject, due).await
    })
    .await
}

/// Schedules a job that did not exist.
async fn scheduled(pool: &SqlPool, kind: Deadline, name: &str, due: DateTime<Utc>) {
    assert_eq!(
        schedule(pool, kind, name, due).await.expect("scheduled"),
        Scheduled::Created
    );
}

#[tokio::test]
async fn scheduling_the_same_job_twice_leaves_one_row() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::days(2);

    let first = schedule(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let second = schedule(&pool, Deadline::HoldLapse, "hold/1", due).await;

    assert_eq!(first.expect("scheduled"), Scheduled::Created);
    assert_eq!(second.expect("scheduled"), Scheduled::Unchanged);
    assert_eq!(count_jobs(&pool).await, 1);
    let stored = job(&pool, Deadline::HoldLapse, &subject("hold/1"))
        .await
        .expect("the job");
    assert_eq!(stored.due_at(), due.trunc_subsecs(6));
    assert_eq!(stored.next_attempt_at(), stored.due_at());
}

#[tokio::test]
async fn scheduling_at_a_new_due_time_moves_the_pending_job() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::days(2);
    let later = due + TimeDelta::days(1);

    scheduled(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let moved = schedule(&pool, Deadline::HoldLapse, "hold/1", later)
        .await
        .expect("moved");

    assert_eq!(
        moved,
        Scheduled::Moved {
            from: due.trunc_subsecs(6)
        }
    );
    assert_eq!(count_jobs(&pool).await, 1);
}

#[tokio::test]
async fn one_subject_holds_one_job_per_kind() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::days(2);

    for kind in [Deadline::HoldLapse, Deadline::Reminder] {
        assert_eq!(
            schedule(&pool, kind, "hold/1", due)
                .await
                .expect("scheduled"),
            Scheduled::Created
        );
    }
    assert_eq!(count_jobs(&pool).await, 2);
}

#[tokio::test]
async fn a_reschedule_is_one_statement() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::days(2);
    scheduled(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let hold = subject("hold/1");

    let (outcome, statements) =
        counting_statements(pool.transaction(IsolationLevel::Serializable, async |tx| {
            tx.reschedule(Deadline::HoldLapse, &hold, due + TimeDelta::hours(3))
                .await
        }))
        .await;

    assert_eq!(outcome.expect("rescheduled"), Rescheduled::Moved);
    assert_eq!(statements, 1, "a reschedule is exactly one statement");
    let stored = job(&pool, Deadline::HoldLapse, &hold)
        .await
        .expect("the job");
    assert_eq!(
        stored.due_at(),
        (due + TimeDelta::hours(3)).trunc_subsecs(6)
    );
}

#[tokio::test]
async fn a_cancel_is_one_statement() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::days(2);
    scheduled(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let hold = subject("hold/1");

    let (outcome, statements) =
        counting_statements(pool.transaction(IsolationLevel::Serializable, async |tx| {
            tx.cancel(Deadline::HoldLapse, &hold).await
        }))
        .await;

    assert_eq!(outcome.expect("cancelled"), Cancelled::Removed);
    assert_eq!(statements, 1, "a cancel is exactly one statement");
    assert_eq!(count_jobs(&pool).await, 0);
}

#[tokio::test]
async fn a_reschedule_or_cancel_of_nothing_says_so() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let hold = subject("hold/1");
    let due = database_now(&pool).await + TimeDelta::days(2);

    let (moved, cancelled) = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let moved = tx.reschedule(Deadline::HoldLapse, &hold, due).await?;
            let cancelled = tx.cancel(Deadline::HoldLapse, &hold).await?;
            Ok::<_, JobError>((moved, cancelled))
        })
        .await
        .expect("answered");

    assert_eq!(moved, Rescheduled::NotPending);
    assert_eq!(cancelled, Cancelled::NotPending);
}

#[tokio::test]
async fn a_fired_occurrence_is_reported_and_never_moved_or_cancelled() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    scheduled(&pool, Deadline::HoldLapse, "hold/1", due).await;
    drain_until_idle(&drain(&pool, Recording::new())).await;
    let hold = subject("hold/1");
    let fired_at = job(&pool, Deadline::HoldLapse, &hold)
        .await
        .and_then(|job| job.fired_at())
        .expect("it fired");

    let again = schedule(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let (moved, cancelled) = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let moved = tx
                .reschedule(Deadline::HoldLapse, &hold, due + TimeDelta::days(1))
                .await?;
            let cancelled = tx.cancel(Deadline::HoldLapse, &hold).await?;
            Ok::<_, JobError>((moved, cancelled))
        })
        .await
        .expect("answered");

    assert_eq!(
        again.expect("answered"),
        Scheduled::AlreadyFired { fired_at }
    );
    assert_eq!(moved, Rescheduled::NotPending);
    assert_eq!(cancelled, Cancelled::NotPending);
    assert_eq!(support::effects(&pool).await.len(), 1);
}

#[tokio::test]
async fn a_fired_job_is_never_armed_again_even_for_a_new_due_time() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    let first_due = now - TimeDelta::seconds(2);
    scheduled(&pool, Deadline::AutoClose, "pin/9", first_due).await;
    let drain = drain(&pool, Recording::new());
    drain_until_idle(&drain).await;

    let later = schedule(
        &pool,
        Deadline::AutoClose,
        "pin/9",
        now - TimeDelta::seconds(1),
    )
    .await
    .expect("answered");
    let replayed = schedule(&pool, Deadline::AutoClose, "pin/9", first_due)
        .await
        .expect("answered");
    drain_until_idle(&drain).await;

    assert!(matches!(later, Scheduled::AlreadyFired { .. }), "{later:?}");
    assert!(
        matches!(replayed, Scheduled::AlreadyFired { .. }),
        "{replayed:?}"
    );
    assert_eq!(support::effects_for(&pool, "pin/9").await, 1);
    assert_eq!(count_jobs(&pool).await, 1);
}

#[tokio::test]
async fn a_due_time_older_than_the_retention_window_is_refused() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let long_ago = database_now(&pool).await - TimeDelta::days(8);

    let error = schedule(&pool, Deadline::HoldLapse, "hold/1", long_ago)
        .await
        .expect_err("refused");

    assert_eq!(error.error_code(), "JOBS_003");
    assert_eq!(count_jobs(&pool).await, 0);
}

#[tokio::test]
async fn a_reschedule_to_before_the_retention_window_is_refused() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let now = database_now(&pool).await;
    scheduled(
        &pool,
        Deadline::HoldLapse,
        "hold/1",
        now + TimeDelta::days(1),
    )
    .await;
    let hold = subject("hold/1");

    let error = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            tx.reschedule(Deadline::HoldLapse, &hold, now - TimeDelta::days(8))
                .await
        })
        .await
        .expect_err("refused");

    assert_eq!(error.error_code(), "JOBS_003");
}

#[tokio::test]
async fn a_job_written_in_a_rolled_back_transaction_never_fires() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    let hold = subject("hold/1");

    let rolled_back = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let _created = tx.schedule(Deadline::HoldLapse, &hold, due).await?;
            Err::<(), JobError>(JobError::Contended {
                kind: "hold.lapse".to_owned(),
            })
        })
        .await;
    drain_until_idle(&drain(&pool, Recording::new())).await;

    assert!(matches!(rolled_back, Err(JobError::Contended { .. })));
    assert_eq!(count_jobs(&pool).await, 0);
    assert!(support::effects(&pool).await.is_empty());
}

#[tokio::test]
async fn a_job_written_in_a_committed_transaction_fires_once() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await - TimeDelta::seconds(1);
    scheduled(&pool, Deadline::HoldLapse, "hold/1", due).await;
    let drain = drain(&pool, Recording::new());

    drain_until_idle(&drain).await;
    drain_until_idle(&drain).await;

    let effects = support::effects(&pool).await;
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].subject, "hold/1");
    assert!(effects[0].fired_at >= effects[0].due_at);
}

/// Runs `work` and counts the statements it ran inside its transaction.
async fn counting_statements<T>(work: impl std::future::Future<Output = T>) -> (T, usize) {
    let counted = Arc::new(AtomicUsize::new(0));
    let subscriber = Registry::default().with(StatementCounter(Arc::clone(&counted)));
    let outcome = work.with_subscriber(subscriber).await;
    (outcome, counted.load(Ordering::SeqCst))
}

/// Counts the spans a transaction's statements open — each carries `op = "tx"`.
struct StatementCounter(Arc<AtomicUsize>);

impl<S: Subscriber> Layer<S> for StatementCounter {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let mut op = OpField(false);
        attrs.record(&mut op);
        if op.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

struct OpField(bool);

impl Visit for OpField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "op" && value == "tx" {
            self.0 = true;
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

#[tokio::test]
async fn no_job_span_records_a_subject() {
    let (_schema, pool) = support::schema_with_jobs().await;
    let due = database_now(&pool).await + TimeDelta::hours(1);
    let occurrence = subject("mailbox/occurrence-4242");
    let values = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let subscriber = Registry::default().with(SpanValues(Arc::clone(&values)));
    let (moved, cancelled) = pool
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            let _: Scheduled = tx.schedule(Deadline::Reminder, &occurrence, due).await?;
            let moved = tx
                .reschedule(Deadline::Reminder, &occurrence, due + TimeDelta::hours(1))
                .await?;
            let cancelled = tx.cancel(Deadline::Reminder, &occurrence).await?;
            Ok::<_, JobError>((moved, cancelled))
        })
        .with_subscriber(subscriber)
        .await
        .expect("scheduled, moved and cancelled");
    assert_eq!(moved, Rescheduled::Moved);
    assert_eq!(cancelled, Cancelled::Removed);

    let values = values.lock().expect("the recorded values");
    assert!(
        values.iter().any(|value| value.contains("reminder")),
        "the jobs spans were recorded: {values:?}"
    );
    assert!(
        values
            .iter()
            .all(|value| !value.contains("occurrence-4242")),
        "a span recorded the subject: {values:?}"
    );
}

/// Every field value of every `jobs.*` span, as text.
struct SpanValues(Arc<std::sync::Mutex<Vec<String>>>);

impl<S: Subscriber> Layer<S> for SpanValues {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name().starts_with("jobs.") {
            let mut collected = Collected(Vec::new());
            attrs.record(&mut collected);
            if let Ok(mut values) = self.0.lock() {
                values.extend(collected.0);
            }
        }
    }
}

struct Collected(Vec<String>);

impl Visit for Collected {
    fn record_str(&mut self, _field: &Field, value: &str) {
        self.0.push(value.to_owned());
    }

    fn record_debug(&mut self, _field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push(format!("{value:?}"));
    }
}
