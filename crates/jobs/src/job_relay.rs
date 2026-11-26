use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::SqlDb;
use fundamental_db::{
    Filter, FilterValue, KeyedRepositoryTransaction, Relay, RepositoryTransactionQuery, RetryPolicy,
};
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use tracing::field::{Empty, display};
use tracing::{Span, info, instrument, warn};

use crate::kind_set::{HandlerSet, KindSet};
use crate::schedule_jobs::{due_assignments, in_range, key};
use crate::{
    Fire, FireOutcome, FiringJob, JobError, JobHandler, JobId, JobKind, JobSubject, JobsSettings,
    ScheduledJob,
};

/// Fires one claimed job: the [`Relay`] a job table is drained through.
///
/// The fired mark is written only while the row still carries this claim's lease and has not
/// fired, so a job's effect commits exactly once.
pub struct JobRelay<K, H> {
    pool: SqlPool,
    sets: Arc<[Arc<dyn KindSet>]>,
    defer: Duration,
    _served: PhantomData<fn() -> (K, H)>,
}

impl<K, H> JobRelay<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    /// A relay firing jobs through `handler` on `pool`, deferring by `settings`' cadence.
    pub fn new(pool: SqlPool, handler: Arc<H>, settings: &JobsSettings) -> Self {
        Self::serving(
            pool,
            vec![Arc::new(HandlerSet::<K, H>::new(handler))],
            settings.defer(),
        )
    }

    /// A relay firing each of `sets`' kinds through its own handler, deferring by `defer`.
    pub(crate) fn serving(pool: SqlPool, sets: Vec<Arc<dyn KindSet>>, defer: Duration) -> Self {
        Self {
            pool,
            sets: sets.into(),
            defer,
            _served: PhantomData,
        }
    }

    /// The sets this relay serves.
    pub(crate) fn sets(&self) -> Vec<Arc<dyn KindSet>> {
        self.sets.to_vec()
    }

    /// Fires `job`, answering what the attempt found.
    ///
    /// # Errors
    ///
    /// [`JobError::UnknownKind`] or [`JobError::IdentityMismatch`] for a row this application
    /// cannot have written; [`JobError::HandlerFailed`] when the handler refused, and
    /// [`JobError::PostponeRefused`] when it postponed to an instant it may not, each having rolled
    /// its writes back; [`JobError::VerdictNotRecorded`] when its verdict could not be written; a
    /// store error otherwise.
    #[instrument(name = "jobs.fire", skip(self, job), fields(job_id = %job.job_id(), kind = %job.kind, bucket = job.bucket, due_at = %job.due_at, attempt = job.attempts + 1, lateness_ms = Empty, outcome = Empty, not_before = Empty, next_attempt_at = Empty), err(Debug))]
    pub async fn fire(&self, job: &ScheduledJob) -> Result<FireOutcome, JobError> {
        let set = self
            .sets
            .iter()
            .find(|set| set.kinds().contains(&job.kind.as_str()))
            .ok_or_else(|| JobError::UnknownKind {
                kind: job.kind.clone(),
            })?;
        set.fire(&self.pool, job, self.defer).await
    }
}

impl<K, H> Relay<ScheduledJob> for JobRelay<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    async fn relay(&self, row: &ScheduledJob) -> Result<(), AnyError> {
        self.fire(row).await.map(|_| ()).map_err(AnyError::new)
    }

    /// The defer cadence, folded into the policy the drain claims and inventories under.
    fn recurrence(&self) -> Option<Duration> {
        Some(self.defer)
    }
}

/// What the claim handed over: the job, the lease it wrote, and the due time it saw.
#[derive(Clone, Copy)]
pub(crate) struct Claimed {
    pub(crate) job_id: JobId,
    pub(crate) token: DateTime<Utc>,
    pub(crate) due_at: DateTime<Utc>,
    pub(crate) failed_attempts: i64,
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    reason = "readability baseline"
)]
pub(crate) async fn fire_in<K, H>(
    tx: &mut SqlTransaction,
    handler: &H,
    kind: K,
    subject: &JobSubject,
    claimed: Claimed,
    defer: Duration,
) -> Result<FireOutcome, JobError>
where
    K: JobKind,
    H: JobHandler<K>,
{
    let now = ClusterTime::read_in(tx)
        .await
        .map_err(|cause| JobError::ClockUnavailable { cause })?
        .to_utc();
    Span::current().record("lateness_ms", (now - claimed.due_at).num_milliseconds());
    let mark = key(claimed.job_id)
        .filter(Filter::is_null(ScheduledJob::FIRED_AT))
        .filter(Filter::eq(ScheduledJob::NEXT_ATTEMPT_AT, claimed.token))
        .filter(Filter::eq(ScheduledJob::DUE_AT, claimed.due_at))
        .filter(Filter::lte(ScheduledJob::DUE_AT, now));
    let marked = tx
        .update_where(&mark, &[(ScheduledJob::FIRED_AT.to_owned(), now.into())])
        .await?;
    if marked == 1 {
        let job = FiringJob {
            kind,
            subject: subject.clone(),
            due_at: claimed.due_at,
            fired_at: now,
            failed_attempts: claimed.failed_attempts,
        };
        let verdict = match handler.fire(tx, &job).await {
            Ok(verdict) => verdict,
            Err(cause) => {
                let handler_code = cause.get().error_code().to_owned();
                warn!(handler_code, "job handler failed; its writes roll back");
                return Err(JobError::HandlerFailed {
                    kind: kind.value().to_owned(),
                    handler_code,
                    cause,
                });
            }
        };
        let marked = Marked {
            job_id: claimed.job_id,
            due_at: claimed.due_at,
            at: now,
        };
        return record_verdict(tx, kind, marked, verdict, defer).await;
    }
    let current =
        KeyedRepositoryTransaction::<ScheduledJob, SqlDb>::find(tx, &claimed.job_id.as_uuid())
            .await?;
    Ok(match current {
        None => FireOutcome::Cancelled,
        Some(current) if current.fired_at.is_some() => FireOutcome::AlreadyFired,
        Some(current)
            if current.next_attempt_at != claimed.token || current.due_at != claimed.due_at =>
        {
            FireOutcome::Superseded
        }
        Some(current) => {
            // A replica whose clock runs ahead claimed it early: put it back without counting a
            // failure.
            let still_ours = key(claimed.job_id)
                .filter(Filter::is_null(ScheduledJob::FIRED_AT))
                .filter(Filter::eq(ScheduledJob::NEXT_ATTEMPT_AT, claimed.token));
            tx.update_where(
                &still_ours,
                &[(
                    ScheduledJob::NEXT_ATTEMPT_AT.to_owned(),
                    current.due_at.into(),
                )],
            )
            .await?;
            FireOutcome::NotYetDue
        }
    })
}

/// The fired mark this transaction wrote, which a verdict is written against.
#[derive(Clone, Copy)]
struct Marked {
    job_id: JobId,
    due_at: DateTime<Utc>,
    at: DateTime<Utc>,
}

/// Writes `verdict` against this transaction's fired mark, so it lands only on the row it holds.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn record_verdict<K: JobKind>(
    tx: &mut SqlTransaction,
    kind: K,
    marked: Marked,
    verdict: Fire,
    defer: Duration,
) -> Result<FireOutcome, JobError> {
    let (outcome, assignments) = match verdict {
        Fire::Done => return Ok(FireOutcome::Fired),
        Fire::NotBefore(at) => {
            let at = postponed_to(kind, at, marked.at)?;
            Span::current().record("not_before", display(at));
            info!(
                not_before = %at,
                postponed_by_ms = (at - marked.due_at).num_milliseconds(),
                "job postponed by its handler"
            );
            let mut assignments = vec![(ScheduledJob::FIRED_AT.to_owned(), FilterValue::Null)];
            assignments.extend(due_assignments(at));
            (FireOutcome::Postponed, assignments)
        }
        Fire::Defer => {
            // Measured after the handler returned, so a slow handler cannot commit a past next
            // attempt.
            let answered = ClusterTime::read_in(tx)
                .await
                .map_err(|cause| JobError::ClockUnavailable { cause })?
                .to_utc();
            let next = deferred_until(marked.job_id, answered, defer).ok_or_else(|| {
                JobError::VerdictNotRecorded {
                    job_id: marked.job_id.to_string(),
                    reason: "the next attempt is past any representable instant",
                }
            })?;
            Span::current().record("next_attempt_at", display(next));
            info!(next_attempt_at = %next, "job deferred by its handler");
            (
                FireOutcome::Deferred,
                vec![
                    (ScheduledJob::FIRED_AT.to_owned(), FilterValue::Null),
                    (ScheduledJob::NEXT_ATTEMPT_AT.to_owned(), next.into()),
                ],
            )
        }
    };
    let ours = key(marked.job_id).filter(Filter::eq(ScheduledJob::FIRED_AT, marked.at));
    let written = tx.update_where(&ours, &assignments).await?;
    if written != 1 {
        return Err(JobError::VerdictNotRecorded {
            job_id: marked.job_id.to_string(),
            reason: "the job no longer carries this transaction's fired mark",
        });
    }
    Ok(outcome)
}

/// `at` as a postponed due time: in range, strictly after `now`, and rounded up to the
/// microsecond, since rounding a "not before" down fires it early.
fn postponed_to<K: JobKind>(
    kind: K,
    at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, JobError> {
    let refused = |reason| JobError::PostponeRefused {
        kind: kind.value().to_owned(),
        not_before: at.to_rfc3339(),
        reason,
    };
    let whole = at.trunc_subsecs(6);
    let at = if whole < at {
        whole.checked_add_signed(TimeDelta::microseconds(1))
    } else {
        Some(whole)
    }
    .ok_or_else(|| refused("outside the years 2000 to 2200"))?;
    let at = in_range(kind, at).map_err(|_| refused("outside the years 2000 to 2200"))?;
    if at <= now {
        return Err(refused("not after the database's clock"));
    }
    Ok(at)
}

/// When a job deferred at `answered` is asked again: one `defer` later, less a jitter of up to a
/// quarter.
fn deferred_until(
    job_id: JobId,
    answered: DateTime<Utc>,
    defer: Duration,
) -> Option<DateTime<Utc>> {
    let jittered = RetryPolicy::new(Duration::ZERO, defer, defer).delay_for(1, &job_id.to_string());
    answered.checked_add_signed(TimeDelta::from_std(jittered).ok()?)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use fundamental_core::Error as _;
    use fundamental_macros::Variant;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
    enum Deadline {
        #[variant(value = "hold.lapse")]
        HoldLapse,
    }

    impl JobKind for Deadline {}

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0)
            .single()
            .expect("an instant")
    }

    #[test]
    fn a_postpone_must_lie_after_the_database_clock() {
        let later = now() + TimeDelta::microseconds(1);
        assert_eq!(
            postponed_to(Deadline::HoldLapse, later, now()).expect("later"),
            later
        );
        for refused in [now(), now() - TimeDelta::seconds(1)] {
            let error = postponed_to(Deadline::HoldLapse, refused, now()).expect_err("refused");
            assert_eq!(error.error_code(), "JOBS_013");
            assert!(
                error
                    .developer_message()
                    .is_some_and(|message| message.contains("not after the database's clock")),
                "{error:?}"
            );
        }
    }

    #[test]
    fn a_postpone_past_the_supported_years_is_refused() {
        let far = Utc
            .with_ymd_and_hms(2200, 1, 1, 0, 0, 0)
            .single()
            .expect("an instant");
        let error = postponed_to(Deadline::HoldLapse, far, now()).expect_err("refused");
        assert_eq!(error.error_code(), "JOBS_013");
        assert!(
            error
                .developer_message()
                .is_some_and(|message| message.contains("2000 to 2200")),
            "{error:?}"
        );
    }

    #[test]
    fn a_postpone_is_rounded_up_to_the_whole_microsecond() {
        let at = now() + TimeDelta::nanoseconds(1_500);
        let kept = postponed_to(Deadline::HoldLapse, at, now()).expect("later");
        assert_eq!(kept, now() + TimeDelta::microseconds(2));
    }

    #[test]
    fn a_postpone_less_than_a_microsecond_ahead_is_still_later() {
        let at = now() + TimeDelta::nanoseconds(1);
        let kept = postponed_to(Deadline::HoldLapse, at, now()).expect("later");
        assert_eq!(kept, now() + TimeDelta::microseconds(1));
    }

    #[test]
    fn a_deferral_lands_between_three_quarters_of_and_one_cadence() {
        let defer = Duration::from_secs(30);
        let span = TimeDelta::seconds(30);
        let mut distinct = std::collections::BTreeSet::new();
        for index in 0..64 {
            let subject = JobSubject::new(format!("hold/{index}")).expect("a subject");
            let next = deferred_until(JobId::of(Deadline::HoldLapse, &subject), now(), defer)
                .expect("representable");
            assert!(next >= now() + span * 3 / 4);
            assert!(next <= now() + span);
            distinct.insert(next);
        }
        assert!(
            distinct.len() > 32,
            "a cohort deferred at one instant spreads out: {} distinct",
            distinct.len()
        );
    }

    #[test]
    fn a_deferral_past_any_instant_is_not_representable() {
        let job = JobId::of(
            Deadline::HoldLapse,
            &JobSubject::new("hold/1").expect("a subject"),
        );
        assert_eq!(
            deferred_until(job, DateTime::<Utc>::MAX_UTC, Duration::from_secs(30)),
            None
        );
    }
}
