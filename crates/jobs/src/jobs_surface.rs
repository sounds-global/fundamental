use std::marker::PhantomData;
use std::sync::Arc;

use fundamental_core::AnyError;
use fundamental_runtime::{
    DrainFuture, OutboxObserver, OutboxSurface, PeriodicSurface, StartFuture, Surface,
    SurfaceGuard, WaitFuture,
};
use fundamental_sql::SqlPool;
use tracing::info;

use crate::job_kind::{validate_disjoint, validate_kinds};
use crate::kind_set::{HandlerSet, KindSet};
use crate::{JobBucket, JobError, JobHandler, JobKind, JobsDrain, JobsPurge, JobsSettings};

/// Fires an application's durable jobs on the runtime, each once through its [`JobHandler`].
#[must_use = "a JobsSurface does nothing until handed to a Runtime"]
pub struct JobsSurface<K, H> {
    name: &'static str,
    pool: SqlPool,
    sets: Vec<Arc<dyn KindSet>>,
    settings: JobsSettings,
    observer: Option<Arc<dyn OutboxObserver>>,
    _served: PhantomData<fn() -> (K, H)>,
}

impl<K, H> JobsSurface<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    /// A surface named `name` firing `K`'s jobs on `pool` through `handler`, under `settings`.
    ///
    /// `pool` must hold the outbox seal key, as every pool that writes an outbox row does.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidKinds`] when `K` declares no kind, a malformed one, or one twice;
    /// [`JobError::InvalidSettings`] with every problem in `settings`.
    pub fn new(
        name: &'static str,
        pool: SqlPool,
        handler: Arc<H>,
        settings: JobsSettings,
    ) -> Result<Self, JobError> {
        validate_kinds::<K>()?;
        let settings = settings.checked()?;
        Ok(Self {
            name,
            pool,
            sets: vec![Arc::new(HandlerSet::<K, H>::new(handler))],
            settings,
            observer: None,
            _served: PhantomData,
        })
    }

    /// This surface also firing `K2`'s jobs through `handler`, at `H2`'s own isolation.
    ///
    /// # Errors
    ///
    /// [`JobError::InvalidKinds`] when `K2` declares no kind, a malformed one, one twice, or one
    /// a set this surface already serves declares.
    pub fn serving<K2, H2>(mut self, handler: Arc<H2>) -> Result<Self, JobError>
    where
        K2: JobKind,
        H2: JobHandler<K2>,
    {
        validate_kinds::<K2>()?;
        let served: Vec<&'static str> = self
            .sets
            .iter()
            .flat_map(|set| set.kinds().iter().copied())
            .collect();
        validate_disjoint::<K2>(&served)?;
        self.sets.push(Arc::new(HandlerSet::<K2, H2>::new(handler)));
        Ok(self)
    }

    /// Reports every drain tick and inventory to `observer`.
    pub fn with_observer(mut self, observer: Arc<dyn OutboxObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl<K, H> Surface for JobsSurface<K, H>
where
    K: JobKind,
    H: JobHandler<K>,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn start(self: Box<Self>) -> StartFuture {
        Box::pin(async move {
            let Self {
                name,
                pool,
                sets,
                settings,
                observer,
                _served,
            } = *self;
            let kinds: Vec<&'static str> = sets
                .iter()
                .flat_map(|set| set.kinds().iter().copied())
                .collect();
            let drain = Arc::new(
                JobsDrain::<K, H>::from_sets(pool.clone(), sets, settings)
                    .map_err(AnyError::new)?,
            );
            let mut drain = OutboxSurface::new(
                name,
                drain,
                settings.poll_interval(),
                u64::from(settings.batch_per_bucket),
            )
            .with_inventory_interval(settings.inventory_interval());
            if let Some(observer) = observer {
                drain = drain.with_observer(observer);
            }
            let purge = PeriodicSurface::new(
                name,
                Arc::new(JobsPurge::new(pool, settings).map_err(AnyError::new)?),
                settings.purge_interval(),
            )
            .after_first_interval();
            let drain = Box::new(drain).start().await?;
            let purge = match Box::new(purge).start().await {
                Ok(purge) => purge,
                Err(error) => {
                    drain.stop().await?;
                    return Err(error);
                }
            };
            info!(
                surface = name,
                ?kinds,
                buckets = JobBucket::COUNT,
                "jobs surface started"
            );
            Ok(Box::new(JobsGuard { name, drain, purge }) as Box<dyn SurfaceGuard>)
        })
    }
}

/// The drain's guard and the purge's, run and stopped as one.
struct JobsGuard {
    name: &'static str,
    drain: Box<dyn SurfaceGuard>,
    purge: Box<dyn SurfaceGuard>,
}

impl SurfaceGuard for JobsGuard {
    fn name(&self) -> &'static str {
        self.name
    }

    fn wait(&mut self) -> WaitFuture<'_> {
        let Self { drain, purge, .. } = self;
        Box::pin(async move {
            tokio::select! {
                ended = drain.wait() => ended,
                ended = purge.wait() => ended,
            }
        })
    }

    fn stop(self: Box<Self>) -> DrainFuture {
        Box::pin(async move {
            let Self { drain, purge, .. } = *self;
            let drained = drain.stop().await;
            let purged = purge.stop().await;
            drained.and(purged).map_err(|error: AnyError| error)
        })
    }
}
