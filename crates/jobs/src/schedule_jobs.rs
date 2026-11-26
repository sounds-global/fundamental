use std::future::Future;

use chrono::{DateTime, SubsecRound, TimeZone, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{
    Filter, FilterValue, KeyedRepositoryTransaction, QueryBuilder, RepositoryTransaction,
    RepositoryTransactionQuery,
};
use fundamental_sql::{ClusterTime, SqlQuery, SqlTransaction};
use tracing::{Span, debug, field::Empty, instrument};

use crate::{
    Cancelled, Held, JobError, JobId, JobKind, JobSubject, JobsSettings, Rescheduled, Scheduled,
    ScheduledJob,
};

/// How many times [`schedule`](ScheduleJobs::schedule) re-reads a job another writer changed
/// under it before answering [`JobError::Contended`].
const SCHEDULE_ATTEMPTS: usize = 3;

/// Schedules, moves and cancels durable jobs inside the caller's transaction, so a deadline
/// commits or rolls back with the state that caused it.
pub trait ScheduleJobs {
    /// Ensures the job of `kind` about `subject` is due at `due`.
    ///
    /// A pending job already at `due` is [`Scheduled::Unchanged`]; a job that fired is
    /// [`Scheduled::AlreadyFired`] and never armed again. `due` is kept to whole microseconds.
    ///
    /// # Errors
    ///
    /// [`JobError::DueOutOfRange`] for a due time outside the years 2000 to 2200, or older than
    /// [`JobsSettings::MIN_RETENTION_HOURS`] by the database's clock;
    /// [`JobError::ClockUnavailable`] when that clock cannot be read; [`JobError::Contended`] when
    /// other writers keep changing the job; a store error otherwise.
    fn schedule<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
        due: DateTime<Utc>,
    ) -> impl Future<Output = Result<Scheduled, JobError>> + Send;

    /// Moves the pending job of `kind` about `subject` to `due`, resetting its attempts.
    ///
    /// # Errors
    ///
    /// [`JobError::DueOutOfRange`] outside the years 2000 to 2200 or older than
    /// [`JobsSettings::MIN_RETENTION_HOURS`]; [`JobError::ClockUnavailable`] when the database's
    /// clock cannot be read; a store error otherwise.
    fn reschedule<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
        due: DateTime<Utc>,
    ) -> impl Future<Output = Result<Rescheduled, JobError>> + Send;

    /// Removes the pending job of `kind` about `subject`; a fired job's record is kept.
    ///
    /// # Errors
    ///
    /// A store error.
    fn cancel<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
    ) -> impl Future<Output = Result<Cancelled, JobError>> + Send;

    /// Locks the pending job of `kind` about `subject` until the transaction ends, then reads the
    /// database's clock to say whether it is due.
    ///
    /// Call it before locking anything the handler locks, so a job falling due meanwhile waits for
    /// this transaction instead of deadlocking with it.
    ///
    /// # Errors
    ///
    /// [`JobError::ClockUnavailable`] when the database's clock cannot be read; a store error
    /// otherwise.
    fn hold<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
    ) -> impl Future<Output = Result<Held, JobError>> + Send;
}

impl ScheduleJobs for SqlTransaction {
    #[instrument(name = "jobs.schedule", skip(self, subject), fields(kind = kind.value(), job_id = %JobId::of(kind, subject), %due, outcome = Empty), err(Debug))]
    async fn schedule<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
        due: DateTime<Utc>,
    ) -> Result<Scheduled, JobError> {
        let due = in_range(kind, due)?;
        after_the_retention_floor(self, kind, due).await?;
        let job = ScheduledJob::pending(kind.value(), subject.as_str(), due);
        for attempt in 1..=SCHEDULE_ATTEMPTS {
            if self
                .insert_if_absent(&job)
                .await
                .map_err(|error| JobError::from(error).about())?
                .is_some()
            {
                return Ok(answered(Scheduled::Created));
            }
            let Some(current) =
                KeyedRepositoryTransaction::<ScheduledJob, SqlDb>::find(self, &job.job_id)
                    .await
                    .map_err(|error| JobError::from(error).about())?
            else {
                debug!(
                    attempt,
                    "job removed between insert and read; scheduling again"
                );
                continue;
            };
            // A fired job is never armed again: a replayed schedule of an earlier due time would
            // otherwise fire that occurrence a second time.
            if let Some(fired_at) = current.fired_at {
                return Ok(answered(Scheduled::AlreadyFired { fired_at }));
            }
            if current.due_at == due {
                return Ok(answered(Scheduled::Unchanged));
            }
            // Fenced on what was read, so a concurrent writer is noticed rather than overwritten.
            let fenced = key(job.job_id())
                .filter(Filter::eq(ScheduledJob::DUE_AT, current.due_at))
                .filter(Filter::is_null(ScheduledJob::FIRED_AT));
            let moved = self
                .update_where(&fenced, &due_assignments(due))
                .await
                .map_err(|error| JobError::from(error).about())?;
            if moved == 1 {
                return Ok(answered(Scheduled::Moved {
                    from: current.due_at,
                }));
            }
            debug!(
                attempt,
                "job changed between read and write; scheduling again"
            );
        }
        Err(JobError::Contended {
            kind: kind.value().to_owned(),
        })
    }

    #[instrument(name = "jobs.reschedule", skip(self, subject), fields(kind = kind.value(), job_id = %JobId::of(kind, subject), %due, outcome = Empty), err(Debug))]
    async fn reschedule<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
        due: DateTime<Utc>,
    ) -> Result<Rescheduled, JobError> {
        let due = in_range(kind, due)?;
        after_the_retention_floor(self, kind, due).await?;
        let pending = key(JobId::of(kind, subject)).filter(Filter::is_null(ScheduledJob::FIRED_AT));
        let moved = self
            .update_where(&pending, &due_assignments(due))
            .await
            .map_err(|error| JobError::from(error).about())?;
        let outcome = if moved == 1 {
            Rescheduled::Moved
        } else {
            Rescheduled::NotPending
        };
        Span::current().record("outcome", format!("{outcome:?}"));
        Ok(outcome)
    }

    #[instrument(name = "jobs.cancel", skip(self, subject), fields(kind = kind.value(), job_id = %JobId::of(kind, subject), outcome = Empty), err(Debug))]
    async fn cancel<K: JobKind>(
        &mut self,
        kind: K,
        subject: &JobSubject,
    ) -> Result<Cancelled, JobError> {
        let pending = key(JobId::of(kind, subject)).filter(Filter::is_null(ScheduledJob::FIRED_AT));
        let removed = self
            .delete_where(&pending)
            .await
            .map_err(|error| JobError::from(error).about())?;
        let outcome = if removed == 1 {
            Cancelled::Removed
        } else {
            Cancelled::NotPending
        };
        Span::current().record("outcome", format!("{outcome:?}"));
        Ok(outcome)
    }

    #[instrument(name = "jobs.hold", skip(self, subject), fields(kind = kind.value(), job_id = %JobId::of(kind, subject), outcome = Empty), err(Debug))]
    async fn hold<K: JobKind>(&mut self, kind: K, subject: &JobSubject) -> Result<Held, JobError> {
        let pending = SqlQuery::new(
            key(JobId::of(kind, subject)).filter(Filter::is_null(ScheduledJob::FIRED_AT)),
        )
        .for_update();
        let held = self
            .query_locked(&pending)
            .await
            .map_err(|error| JobError::from(error).about())?;
        let at = ClusterTime::read_in(self)
            .await
            .map_err(|cause| JobError::ClockUnavailable { cause })?
            .to_utc();
        let outcome = match held.first() {
            Some(job) if job.due_at > at => Held::Pending {
                due_at: job.due_at,
                at,
            },
            Some(job) => Held::Due {
                due_at: job.due_at,
                at,
            },
            None => Held::NotPending { at },
        };
        Span::current().record("outcome", format!("{outcome:?}"));
        Ok(outcome)
    }
}

/// The job table's row for `job_id`, addressed by its whole key.
pub(crate) fn key(job_id: JobId) -> QueryBuilder<ScheduledJob, SqlDb, HasLeadingKey> {
    QueryBuilder::new().pin_key(job_id.as_uuid())
}

/// What makes a pending job wait for `due` afresh: attempted from scratch at its due time.
pub(crate) fn due_assignments(due: DateTime<Utc>) -> [(String, FilterValue); 4] {
    [
        (ScheduledJob::DUE_AT.to_owned(), due.into()),
        (ScheduledJob::NEXT_ATTEMPT_AT.to_owned(), due.into()),
        (ScheduledJob::ATTEMPTS.to_owned(), 0_i64.into()),
        (ScheduledJob::LAST_ERROR.to_owned(), FilterValue::Null),
    ]
}

/// Refuses `due` older than the retention floor by the database's clock.
async fn after_the_retention_floor<K: JobKind>(
    tx: &mut SqlTransaction,
    kind: K,
    due: DateTime<Utc>,
) -> Result<(), JobError> {
    let now = ClusterTime::read_in(tx)
        .await
        .map_err(|cause| JobError::ClockUnavailable { cause })?
        .to_utc();
    if now
        .checked_sub_signed(JobsSettings::min_retention())
        .is_some_and(|floor| due < floor)
    {
        return Err(JobError::DueOutOfRange {
            kind: kind.value().to_owned(),
            due: due.to_rfc3339(),
            reason: "older than the fired-job retention window",
        });
    }
    Ok(())
}

/// `due` to whole microseconds, refused outside the years 2000 to 2200.
pub(crate) fn in_range<K: JobKind>(kind: K, due: DateTime<Utc>) -> Result<DateTime<Utc>, JobError> {
    let earliest = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).single();
    let latest = Utc.with_ymd_and_hms(2200, 1, 1, 0, 0, 0).single();
    let due = due.trunc_subsecs(6);
    match (earliest, latest) {
        (Some(earliest), Some(latest)) if due >= earliest && due < latest => Ok(due),
        _ => Err(JobError::DueOutOfRange {
            kind: kind.value().to_owned(),
            due: due.to_rfc3339(),
            reason: "outside the years 2000 to 2200",
        }),
    }
}

fn answered(outcome: Scheduled) -> Scheduled {
    let name = match outcome {
        Scheduled::Created => "created",
        Scheduled::Moved { .. } => "moved",
        Scheduled::Unchanged => "unchanged",
        Scheduled::AlreadyFired { .. } => "already_fired",
    };
    Span::current().record("outcome", name);
    outcome
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use fundamental_core::Error as _;
    use fundamental_macros::Variant;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
    enum Deadline {
        #[variant(value = "hold.lapse")]
        HoldLapse,
    }

    impl JobKind for Deadline {}

    #[test]
    fn a_due_time_is_kept_to_whole_microseconds() {
        let due = Utc
            .timestamp_opt(1_900_000_000, 123_456_789)
            .single()
            .expect("an instant");
        let kept = in_range(Deadline::HoldLapse, due).expect("in range");
        assert_eq!(kept.timestamp_subsec_nanos(), 123_456_000);
    }

    #[test]
    fn a_due_time_outside_the_supported_years_is_refused() {
        for due in [
            Utc.with_ymd_and_hms(1999, 12, 31, 23, 59, 59).single(),
            Utc.with_ymd_and_hms(2200, 1, 1, 0, 0, 0).single(),
        ] {
            let due = due.expect("an instant");
            let error = in_range(Deadline::HoldLapse, due).expect_err("refused");
            assert_eq!(error.error_code(), "JOBS_003");
        }
    }

    #[test]
    fn the_retention_floor_is_at_least_a_week() {
        assert!(JobsSettings::min_retention() >= TimeDelta::days(7));
    }
}
