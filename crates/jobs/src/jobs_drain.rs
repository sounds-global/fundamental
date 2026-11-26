use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::SqlDb;
use fundamental_core::outbox::{
    DrainReport, HeldCount, OutboxColumns, OutboxInventory, SetAsideBucket,
};
use fundamental_db::{
    DatabaseError, Filter, FilterValue, IsolationLevel, OutboxRow, OutboxSetAside, QueryBuilder,
    Relay, RepositoryTransactionQuery, RetryPolicy, drain_pending,
};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::{ClusterTime, SqlPool};
use futures::stream::{self, StreamExt as _};
use tracing::field::Empty;
use tracing::{Span, debug, info, instrument, warn};
use uuid::Uuid;

use crate::job_kind::{validate_disjoint, validate_kinds};
use crate::kind_set::{HandlerSet, KindSet};
use crate::{
    FireOutcome, JobBucket, JobError, JobHandler, JobKind, JobRelay, JobScope, JobsSettings,
    ScheduledJob,
};

/// How many held rows one bucket's inventory reads before it reports a count as a floor.
const INVENTORY_CAP: u64 = 1_000;

/// Drains the job table for an [`OutboxSurface`](fundamental_runtime::OutboxSurface): every
/// bucket, on the database's clock, a few buckets at once.
pub struct JobsDrain<K, H> {
    pool: SqlPool,
    relay: JobRelay<K, H>,
    scope: JobScope,
    settings: JobsSettings,
    first_bucket: u8,
}

impl<K, H> JobsDrain<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    /// A drain firing `K`'s jobs through `handler`, under `settings`.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidKinds`] when `K`'s declaration is malformed;
    /// [`JobError::InvalidSettings`] with every problem in `settings`.
    pub fn new(pool: SqlPool, handler: Arc<H>, settings: JobsSettings) -> Result<Self, JobError> {
        validate_kinds::<K>()?;
        Self::from_sets(
            pool,
            vec![Arc::new(HandlerSet::<K, H>::new(handler))],
            settings,
        )
    }

    /// This drain also firing `K2`'s jobs through `handler`, claiming the union of every set.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidKinds`] when `K2` declares no kind, a malformed one, one twice, or one
    /// a set this drain already serves declares.
    pub fn serving<K2, H2>(self, handler: Arc<H2>) -> Result<Self, JobError>
    where
        K2: JobKind,
        H2: JobHandler<K2>,
    {
        validate_kinds::<K2>()?;
        validate_disjoint::<K2>(self.scope.kinds())?;
        let mut sets = self.relay.sets();
        sets.push(Arc::new(HandlerSet::<K2, H2>::new(handler)));
        #[cfg(feature = "test-support")]
        let confinement = self.scope.confinement();
        let served = Self::from_sets(self.pool, sets, self.settings)?;
        // A confined drain stays confined whichever order its builder calls come in.
        #[cfg(feature = "test-support")]
        let served = match confinement {
            Some(confinement) => served.confined_to(confinement),
            None => served,
        };
        Ok(served)
    }

    /// A drain firing each of `sets`' kinds through its own handler, under `settings`.
    pub(crate) fn from_sets(
        pool: SqlPool,
        sets: Vec<Arc<dyn KindSet>>,
        settings: JobsSettings,
    ) -> Result<Self, JobError> {
        let settings = settings.checked()?;
        let scope = JobScope::of_kinds(
            sets.iter()
                .flat_map(|set| set.kinds().iter().copied())
                .collect::<Vec<_>>(),
        );
        Ok(Self {
            relay: JobRelay::serving(pool.clone(), sets, settings.defer()),
            pool,
            scope,
            settings,
            first_bucket: Uuid::new_v4().as_bytes()[0],
        })
    }

    /// This drain firing only the jobs about `confinement`'s owner; a confined tick fails when any
    /// bucket fails.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn confined_to(mut self, confinement: crate::JobConfinement) -> Self {
        self.scope = self.scope.confined_to(confinement);
        self
    }

    fn is_confined(&self) -> bool {
        #[cfg(feature = "test-support")]
        return self.scope.confinement().is_some();
        #[cfg(not(feature = "test-support"))]
        return false;
    }

    #[instrument(
        name = "jobs.drain",
        skip(self),
        fields(claimed, fired, postponed, deferred, failed_buckets),
        err(Debug)
    )]
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    async fn drain_all(&self, batch: u64) -> Result<DrainReport, AnyError> {
        let policy = self.retry_policy();
        let concurrency = usize::try_from(self.settings.concurrency).unwrap_or(1);
        let verdicts = Verdicts::default();
        let counting = Counting {
            relay: &self.relay,
            verdicts: &verdicts,
        };
        let counting = &counting;
        let reports: Vec<(JobBucket, Result<DrainReport, AnyError>)> =
            stream::iter(JobBucket::all_from(self.first_bucket))
                .map(|bucket| {
                    let scope = self.scope.in_bucket(bucket);
                    async move {
                        let report = match self.database_now().await {
                            Ok(now) => {
                                drain_pending(&self.pool, counting, &scope, batch, policy, now)
                                    .await
                            }
                            Err(error) => Err(error),
                        };
                        (bucket, report)
                    }
                })
                .buffer_unordered(concurrency)
                .collect()
                .await;

        let mut total = DrainReport::default();
        let mut failed = 0_usize;
        let mut last_error = None;
        for (bucket, report) in reports {
            match report {
                Ok(report) => total = total.plus(&report),
                Err(error) => {
                    warn!(
                        bucket = bucket.get(),
                        code = error.get().error_code(),
                        %error,
                        "a job bucket could not be claimed; the next tick tries it again"
                    );
                    failed += 1;
                    last_error = Some(error);
                }
            }
        }
        let span = Span::current();
        span.record("claimed", total.claimed);
        let (fired, postponed, deferred) = verdicts.totals();
        span.record("fired", fired);
        span.record("postponed", postponed);
        span.record("deferred", deferred);
        // A deferral is an answered attempt, but must not read as a fire every cycle.
        total.relayed = fired;
        if total.claimed > 0 {
            info!(
                claimed = total.claimed,
                fired, postponed, deferred, "job drain tick answered"
            );
        }
        span.record("failed_buckets", failed);
        tick_verdict(self.is_confined(), failed, last_error, total)
    }

    #[instrument(name = "jobs.inventory", skip(self), fields(overdue = Empty), err(Debug))]
    async fn inventory_all(&self) -> Result<OutboxInventory, AnyError> {
        let policy = self.retry_policy();
        let mut set_aside: BTreeMap<SetAsideBucket, HeldCount> = BTreeMap::new();
        let mut beyond_policy = HeldCount::Exact(0);
        let mut skipped = HeldCount::Exact(0);
        let mut overdue = HeldCount::Exact(0);
        for bucket in JobBucket::all_from(0) {
            let now = self.database_now().await?;
            let held = OutboxSetAside::<ScheduledJob, SqlDb>::inventory(
                &self.pool,
                &self.scope.in_bucket(bucket),
                now,
                policy,
                INVENTORY_CAP,
            )
            .await?;
            for (reason, count) in held.buckets() {
                let entry = set_aside.entry(reason).or_insert(HeldCount::Exact(0));
                *entry = entry.plus(count);
            }
            beyond_policy = beyond_policy.plus(held.beyond_policy());
            skipped = skipped.plus(held.skipped());
            self.report_unknown_kinds(bucket, now).await?;
            overdue = overdue.plus(self.count_overdue(bucket, now).await?);
        }
        Span::current().record("overdue", overdue.at_least());
        if !overdue.is_none_held() {
            warn!(
                overdue = overdue.at_least(),
                capped = matches!(overdue, HeldCount::AtLeast(_)),
                overdue_warn_secs = self.settings.overdue_warn_secs,
                known = ?self.scope.kinds(),
                "jobs are pending past their due time: failing, deferring, or not being claimed"
            );
        }
        Ok(OutboxInventory::new(set_aside, beyond_policy, skipped))
    }

    /// Counts this deploy's pending jobs in `bucket` due longer ago than the overdue threshold.
    async fn count_overdue(
        &self,
        bucket: JobBucket,
        now: DateTime<Utc>,
    ) -> Result<HeldCount, AnyError> {
        let Some(threshold) = now.checked_sub_signed(self.settings.overdue_warn()) else {
            return Ok(HeldCount::Exact(0));
        };
        let overdue = <ScheduledJob as OutboxRow<SqlDb>>::pending(&self.scope.in_bucket(bucket))
            .filter(Filter::is_null(OutboxColumns::SET_ASIDE_AT))
            .filter(Filter::is_null(OutboxColumns::SKIP_UNTIL))
            .filter(Filter::lt(ScheduledJob::DUE_AT, threshold));
        self.pool
            .transaction(IsolationLevel::RepeatableRead, async |tx| {
                tx.count_held(&overdue, INVENTORY_CAP).await
            })
            .await
            .map_err(|error: DatabaseError| AnyError::new(JobError::from(error).about()))
    }

    /// Warns when a bucket holds a due job of a kind this deploy does not declare, so never claims.
    async fn report_unknown_kinds(
        &self,
        bucket: JobBucket,
        now: DateTime<Utc>,
    ) -> Result<(), AnyError> {
        let known = self
            .scope
            .kinds()
            .iter()
            .map(|kind| FilterValue::from(*kind))
            .collect();
        let unknown = QueryBuilder::<ScheduledJob, SqlDb>::new()
            .filter(Filter::eq(ScheduledJob::BUCKET, i32::from(bucket.get())))
            .filter(Filter::is_null(ScheduledJob::FIRED_AT))
            .filter(Filter::lte(ScheduledJob::DUE_AT, now))
            .filter(Filter::not_in(ScheduledJob::KIND, known));
        #[cfg(feature = "test-support")]
        let unknown = match self.scope.confinement() {
            Some(confinement) => unknown.filter(confinement.subject_filter(ScheduledJob::SUBJECT)),
            None => unknown,
        };
        let unknown = unknown.limit(1).already_addressed();
        let found = self
            .pool
            .transaction(IsolationLevel::RepeatableRead, async |tx| {
                tx.query_where(&unknown).await
            })
            .await
            .map_err(|error: DatabaseError| AnyError::new(JobError::from(error).about()))?;
        if let Some(job) = found.first() {
            warn!(
                bucket = bucket.get(),
                kind = job.kind(),
                known = ?self.scope.kinds(),
                "due jobs of kinds this deploy does not declare wait for a replica that does"
            );
        }
        Ok(())
    }

    /// The settings' lease and backoff, with the relay's defer cadence.
    fn retry_policy(&self) -> RetryPolicy {
        let policy = self.settings.retry_policy();
        match self.relay.recurrence() {
            Some(period) => policy.with_recurrence(period),
            None => policy,
        }
    }

    async fn database_now(&self) -> Result<DateTime<Utc>, AnyError> {
        let now = ClusterTime::read(&self.pool)
            .await
            .map_err(|cause| AnyError::new(JobError::ClockUnavailable { cause }))?
            .to_utc();
        debug!(%now, "job clock read");
        Ok(now)
    }
}

impl<K, H> OutboxDrain for JobsDrain<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    fn drain(
        &self,
        batch: u64,
    ) -> Pin<Box<dyn Future<Output = Result<DrainReport, AnyError>> + Send + '_>> {
        Box::pin(self.drain_all(batch))
    }

    fn inventory(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<OutboxInventory, AnyError>> + Send + '_>> {
        Box::pin(self.inventory_all())
    }
}

/// A tick fails only when no bucket answered, or, confined, when any bucket did not.
fn tick_verdict(
    confined: bool,
    failed: usize,
    last_error: Option<AnyError>,
    total: DrainReport,
) -> Result<DrainReport, AnyError> {
    let every_bucket = usize::try_from(JobBucket::COUNT).unwrap_or(usize::MAX);
    match last_error {
        Some(error) if confined || failed == every_bucket => Err(error),
        _ => Ok(total),
    }
}

/// What one drain tick's fire attempts answered, by verdict.
#[derive(Default)]
struct Verdicts {
    fired: AtomicUsize,
    postponed: AtomicUsize,
    deferred: AtomicUsize,
}

impl Verdicts {
    fn count(&self, outcome: FireOutcome) {
        let counter = match outcome {
            FireOutcome::Fired => &self.fired,
            FireOutcome::Postponed => &self.postponed,
            FireOutcome::Deferred => &self.deferred,
            FireOutcome::AlreadyFired
            | FireOutcome::Superseded
            | FireOutcome::Cancelled
            | FireOutcome::NotYetDue => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn totals(&self) -> (usize, usize, usize) {
        (
            self.fired.load(Ordering::Relaxed),
            self.postponed.load(Ordering::Relaxed),
            self.deferred.load(Ordering::Relaxed),
        )
    }
}

/// The relay one tick drains through: fires each job and counts what it answered.
struct Counting<'a, K, H> {
    relay: &'a JobRelay<K, H>,
    verdicts: &'a Verdicts,
}

impl<K, H> Relay<ScheduledJob> for Counting<'_, K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    async fn relay(&self, row: &ScheduledJob) -> Result<(), AnyError> {
        let outcome = self.relay.fire(row).await.map_err(AnyError::new)?;
        self.verdicts.count(outcome);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::AnyError;
    use fundamental_core::outbox::DrainReport;

    use super::tick_verdict;
    use crate::{JobBucket, JobError};

    fn refused() -> Option<AnyError> {
        Some(AnyError::new(JobError::TableMissing))
    }

    #[test]
    fn one_failed_bucket_fails_a_confined_tick_only() {
        assert!(tick_verdict(true, 1, refused(), DrainReport::default()).is_err());
        assert!(tick_verdict(false, 1, refused(), DrainReport::default()).is_ok());
    }

    #[test]
    fn every_failed_bucket_fails_any_tick() {
        let every = usize::try_from(JobBucket::COUNT).expect("64 fits");
        assert!(tick_verdict(false, every, refused(), DrainReport::default()).is_err());
    }

    #[test]
    fn a_tick_with_no_failure_answers_its_report() {
        assert!(tick_verdict(true, 0, None, DrainReport::default()).is_ok());
    }
}
