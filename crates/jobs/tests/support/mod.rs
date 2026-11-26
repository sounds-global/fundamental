//! Deadlines of four kinds, and handlers that record each fire as a row in the fire transaction.

// Each suite compiles this module into its own binary and uses a different part of it.
#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db::{
    Filter, KeyedRepository, QueryBuilder, QueryExecutor, RepositoryAggregate,
    RepositoryTransaction,
};
use fundamental_db_migrator::MigrateRunner;
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_jobs::{
    Fire, FiringJob, JOBS_MIGRATIONS, JobHandler, JobId, JobKind, JobSubject, JobsDrain,
    JobsSettings, ScheduledJob,
};
use fundamental_macros::{Error, Model, Variant};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use fundamental_test_containers::sql::SqlSchema;
use tokio::sync::Notify;

/// Four deadlines an app might count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
pub enum Deadline {
    #[variant(value = "hold.lapse")]
    HoldLapse,
    #[variant(value = "bouwcheck.due")]
    BouwcheckDue,
    Reminder,
    #[variant(value = "pin.auto-close")]
    AutoClose,
}

impl JobKind for Deadline {}

/// An older deploy that knows only one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Variant)]
pub enum OldDeadline {
    #[variant(value = "hold.lapse")]
    HoldLapse,
}

impl JobKind for OldDeadline {}

/// One committed effect of one fire.
#[derive(Clone, Debug, Model)]
#[model(sql)]
pub struct Effect {
    #[model(id)]
    pub id: uuid::Uuid,
    pub kind: String,
    pub subject: String,
    pub due_at: DateTime<Utc>,
    pub fired_at: DateTime<Utc>,
    pub sequence: i64,
}

#[derive(Debug, Error)]
pub enum HandlerRefused {
    #[error(
        code = "TEST_JOB_001",
        message = "the hold service refused",
        status = 503
    )]
    Refused,
}

/// Writes one [`Effect`] per fire; parks after writing it when told to, refuses while it has
/// refusals left.
pub struct Recording {
    sequence: AtomicI64,
    refusals: AtomicU32,
    park: Option<Arc<Parking>>,
}

/// Where a parked handler says it has written its effect, and waits forever.
pub struct Parking {
    pub reached: Notify,
}

impl Recording {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sequence: AtomicI64::new(0),
            refusals: AtomicU32::new(0),
            park: None,
        })
    }

    pub fn refusing(times: u32) -> Arc<Self> {
        Arc::new(Self {
            sequence: AtomicI64::new(0),
            refusals: AtomicU32::new(times),
            park: None,
        })
    }

    pub fn parking(parking: Arc<Parking>) -> Arc<Self> {
        Arc::new(Self {
            sequence: AtomicI64::new(0),
            refusals: AtomicU32::new(0),
            park: Some(parking),
        })
    }
}

impl<K: JobKind> JobHandler<K> for Recording {
    async fn fire(&self, tx: &mut SqlTransaction, job: &FiringJob<K>) -> Result<Fire, AnyError> {
        if self
            .refusals
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            return Err(AnyError::new(HandlerRefused::Refused));
        }
        let effect = Effect {
            id: uuid::Uuid::new_v4(),
            kind: job.kind.value().to_owned(),
            subject: job.subject.as_str().to_owned(),
            due_at: job.due_at,
            fired_at: job.fired_at,
            sequence: self.sequence.fetch_add(1, Ordering::SeqCst),
        };
        RepositoryTransaction::insert(tx, &effect)
            .await
            .map_err(AnyError::new)?;
        if let Some(park) = &self.park {
            park.reached.notify_one();
            std::future::pending::<()>().await;
        }
        Ok(Fire::Done)
    }
}

/// One verdict a [`Scripted`] handler answers, decided from the job it is asked about.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    Done,
    /// Postponed this long after the database's clock in the fire transaction.
    PostponeBy(TimeDelta),
    /// Postponed this long after the job's due time — before the clock for a job already due.
    PostponeFromDue(TimeDelta),
    Defer,
    /// Deferred after holding the fire transaction this long — a handler slower than the cadence.
    DeferAfter(std::time::Duration),
}

/// Records every attempt as an [`Effect`], then answers the next scripted verdict; `Done` once the
pub struct Scripted {
    sequence: AtomicI64,
    steps: Mutex<VecDeque<Step>>,
}

impl Scripted {
    pub fn new(steps: impl IntoIterator<Item = Step>) -> Arc<Self> {
        Arc::new(Self {
            sequence: AtomicI64::new(0),
            steps: Mutex::new(steps.into_iter().collect()),
        })
    }

    fn next_step(&self) -> Step {
        self.steps
            .lock()
            .map(|mut steps| steps.pop_front())
            .expect("the script lock")
            .unwrap_or(Step::Done)
    }
}

impl<K: JobKind> JobHandler<K> for Scripted {
    async fn fire(&self, tx: &mut SqlTransaction, job: &FiringJob<K>) -> Result<Fire, AnyError> {
        let effect = Effect {
            id: uuid::Uuid::new_v4(),
            kind: job.kind.value().to_owned(),
            subject: job.subject.as_str().to_owned(),
            due_at: job.due_at,
            fired_at: job.fired_at,
            sequence: self.sequence.fetch_add(1, Ordering::SeqCst),
        };
        RepositoryTransaction::insert(tx, &effect)
            .await
            .map_err(AnyError::new)?;
        Ok(match self.next_step() {
            Step::Done => Fire::Done,
            Step::PostponeBy(span) => Fire::NotBefore(job.fired_at + span),
            Step::PostponeFromDue(span) => Fire::NotBefore(job.due_at + span),
            Step::Defer => Fire::Defer,
            Step::DeferAfter(held) => {
                tokio::time::sleep(held).await;
                Fire::Defer
            }
        })
    }
}

pub fn sealing() -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[0x51; 32])
            .build()
            .expect("an outbox seal key"),
    )
}

/// A fresh schema with the job table applied through the embedded migrations, an effects table,
/// and a pool that seals outbox rows.
pub async fn schema_with_jobs() -> (SqlSchema, SqlPool) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let dir = tempfile::tempdir().expect("a temporary directory");
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "test".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&JOBS_MIGRATIONS)
    .apply_sql(schema.pool())
    .await
    .expect("the job table applies");
    let pool = schema.pool().clone().with_key_manager(sealing());
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS effects \
         (id UUID PRIMARY KEY, kind TEXT NOT NULL, subject TEXT NOT NULL, \
          due_at TIMESTAMPTZ NOT NULL, fired_at TIMESTAMPTZ NOT NULL, sequence BIGINT NOT NULL)",
    )
    .await
    .expect("create effects");
    (schema, pool)
}

/// One-second leases and backoffs, so a lapsed lease or a retry fits inside a test.
pub fn quick() -> JobsSettings {
    JobsSettings {
        lease_secs: 1,
        backoff_base_secs: 1,
        backoff_max_secs: 1,
        concurrency: 4,
        ..JobsSettings::default()
    }
}

pub fn drain<H: JobHandler<Deadline>>(pool: &SqlPool, handler: Arc<H>) -> JobsDrain<Deadline, H> {
    JobsDrain::new(pool.clone(), handler, quick()).expect("a drain")
}

/// Drains until a tick claims nothing.
pub async fn drain_until_idle(drain: &dyn OutboxDrain) {
    for _ in 0..50 {
        let report = drain.drain(100).await.expect("a drain tick");
        if report.claimed == 0 {
            return;
        }
    }
    panic!("the table never went idle");
}

pub fn subject(name: &str) -> JobSubject {
    JobSubject::new(name).expect("a subject")
}

pub async fn effects(pool: &SqlPool) -> Vec<Effect> {
    let mut effects = pool
        .query_executor::<Effect>()
        .execute(&QueryBuilder::new().already_addressed())
        .await
        .expect("read the effects");
    effects.sort_by_key(|effect| effect.sequence);
    effects
}

pub async fn effects_for(pool: &SqlPool, subject: &str) -> usize {
    pool.query_executor::<Effect>()
        .execute(
            &QueryBuilder::new()
                .filter(Filter::eq(Effect::SUBJECT, subject))
                .already_addressed(),
        )
        .await
        .expect("read the effects")
        .len()
}

pub async fn job(pool: &SqlPool, kind: Deadline, subject: &JobSubject) -> Option<ScheduledJob> {
    pool.repository::<ScheduledJob>()
        .find(&JobId::of(kind, subject).as_uuid())
        .await
        .expect("read the job")
}

/// The database's own time.
pub async fn database_now(pool: &SqlPool) -> DateTime<Utc> {
    ClusterTime::read(pool)
        .await
        .expect("the cluster clock")
        .to_utc()
}

pub async fn count_jobs(pool: &SqlPool) -> u64 {
    RepositoryAggregate::count(&pool.repository::<ScheduledJob>())
        .await
        .expect("count the jobs")
}
