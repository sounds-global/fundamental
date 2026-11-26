use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_core::Error as _;
use fundamental_core::model::SqlDb;
use fundamental_db::{
    Filter, FilterValue, IsolationLevel, QueryBuilder, RepositoryTransactionQuery,
};
use fundamental_runtime::PeriodicTask;
use fundamental_sql::{ClusterTime, SqlPool};
use tracing::{Span, info, instrument, warn};

use crate::{JobBucket, JobError, JobsSettings, ScheduledJob};

/// Removes fired jobs' records once they are older than the retention window, a bounded batch
pub struct JobsPurge {
    pool: SqlPool,
    settings: JobsSettings,
}

impl JobsPurge {
    /// A purge of the job table on `pool`, under `settings`.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidSettings`] with every problem in `settings`.
    pub fn new(pool: SqlPool, settings: JobsSettings) -> Result<Self, JobError> {
        Ok(Self {
            pool,
            settings: settings.checked()?,
        })
    }

    /// Purges one batch from every bucket, returning how many records went.
    ///
    /// # Errors
    ///
    /// [`JobError::ClockUnavailable`] when the database's clock cannot be read. A bucket that
    /// cannot be purged is logged and left for the next run.
    #[instrument(name = "jobs.purge", skip(self), fields(purged), err(Debug))]
    pub async fn purge(&self) -> Result<u64, JobError> {
        let now = ClusterTime::read(&self.pool)
            .await
            .map_err(|cause| JobError::ClockUnavailable { cause })?
            .to_utc();
        let Some(cutoff) = now.checked_sub_signed(self.settings.retention()) else {
            return Ok(0);
        };
        let batch = u64::from(self.settings.purge_batch);
        let mut purged = 0_u64;
        for bucket in JobBucket::all_from(0) {
            let fired_before = QueryBuilder::<ScheduledJob, SqlDb>::new()
                .filter(Filter::eq(ScheduledJob::BUCKET, i32::from(bucket.get())))
                .filter(Filter::lt(ScheduledJob::FIRED_AT, cutoff));
            let outcome = self
                .pool
                .transaction(IsolationLevel::RepeatableRead, async |tx| {
                    let expired = tx
                        .query_where(&fired_before.clone().limit(batch).already_addressed())
                        .await?;
                    if expired.is_empty() {
                        return Ok::<u64, JobError>(0);
                    }
                    let ids = expired
                        .iter()
                        .map(|job| FilterValue::from(job.job_id))
                        .collect();
                    let removed = tx
                        .delete_where(
                            &fired_before
                                .clone()
                                .filter(Filter::in_list(ScheduledJob::JOB_ID, ids))
                                .already_addressed(),
                        )
                        .await?;
                    Ok(removed)
                })
                .await
                .map_err(JobError::about);
            match outcome {
                Ok(removed) => purged += removed,
                Err(error) => warn!(
                    bucket = bucket.get(),
                    code = error.error_code(),
                    %error,
                    "fired jobs could not be purged from a bucket; the next run tries again"
                ),
            }
        }
        Span::current().record("purged", purged);
        if purged > 0 {
            info!(purged, %cutoff, "fired job records purged");
        }
        Ok(purged)
    }
}

impl PeriodicTask for JobsPurge {
    fn run(&self) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>> {
        Box::pin(async move { self.purge().await.map(|_| ()).map_err(AnyError::new) })
    }
}
