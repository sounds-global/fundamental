use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use fundamental_sql::SqlPool;
use tracing::{Span, info};

use crate::job_relay::{Claimed, fire_in};
use crate::{FireOutcome, JobError, JobHandler, JobId, JobKind, JobSubject, ScheduledJob};

/// A boxed future answering one fire.
pub(crate) type FireFuture<'a> =
    Pin<Box<dyn Future<Output = Result<FireOutcome, JobError>> + Send + 'a>>;

/// One declared set of kinds and the handler that fires them, behind one object-safe face.
pub(crate) trait KindSet: Send + Sync {
    /// The stored values of every kind this set declares.
    fn kinds(&self) -> &[&'static str];

    /// Fires `job`, whose kind this set declares, in a transaction at the handler's isolation;
    /// a deferral is due again `defer` later.
    fn fire<'a>(
        &'a self,
        pool: &'a SqlPool,
        job: &'a ScheduledJob,
        defer: Duration,
    ) -> FireFuture<'a>;
}

/// The [`KindSet`] of one `JobKind` enum and its handler.
pub(crate) struct HandlerSet<K, H> {
    handler: Arc<H>,
    kinds: Arc<[&'static str]>,
    _kind: PhantomData<fn() -> K>,
}

impl<K, H> HandlerSet<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    /// The set of `K`'s kinds, fired through `handler`.
    pub(crate) fn new(handler: Arc<H>) -> Self {
        Self {
            handler,
            kinds: K::OPTIONS.iter().map(|kind| kind.value()).collect(),
            _kind: PhantomData,
        }
    }
}

impl<K, H> KindSet for HandlerSet<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    fn kinds(&self) -> &[&'static str] {
        &self.kinds
    }

    fn fire<'a>(
        &'a self,
        pool: &'a SqlPool,
        job: &'a ScheduledJob,
        defer: Duration,
    ) -> FireFuture<'a> {
        Box::pin(async move {
            let kind = K::from_value(&job.kind).ok_or_else(|| JobError::UnknownKind {
                kind: job.kind.clone(),
            })?;
            let subject = JobSubject::new(job.subject.clone())?;
            if JobId::of(kind, &subject) != job.job_id() {
                return Err(JobError::IdentityMismatch {
                    job_id: job.job_id().to_string(),
                });
            }
            let claimed = Claimed {
                job_id: job.job_id(),
                token: job.next_attempt_at,
                due_at: job.due_at,
                failed_attempts: job.attempts,
            };
            let handler = &*self.handler;
            let subject = &subject;
            let outcome = pool
                .transaction(H::ISOLATION, async |tx| {
                    fire_in(tx, handler, kind, subject, claimed, defer).await
                })
                .await
                .map_err(JobError::about)?;
            Span::current().record("outcome", outcome.as_str());
            info!(outcome = outcome.as_str(), "job attempt answered");
            Ok(outcome)
        })
    }
}
