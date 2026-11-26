use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use fundamental_api::{AnyError, ApiListener, ApiServer, Router, ServerConfig, ServerListener};
use fundamental_auth::JwtVerifier;
use fundamental_core::secrets::SecretString;
use fundamental_db::migration::{Lints, MigrationConfig, MigrationGuard};
use fundamental_db::{Filter, IsolationLevel, RepositoryTransaction};
use fundamental_db_migrator::MigrateRunner;
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_jobs::{
    Fire, FiringJob, JOBS_MIGRATIONS, JobError, JobHandler, JobSubject, JobsDrain, JobsSettings,
    ScheduleJobs,
};
use fundamental_macros::{action, mount_actions};
use fundamental_nats::{
    AckPolicy, Bytes, ConsumerConfig, DeliverPolicy, JetStream, NatsClient, NatsConfig,
    StreamConfig,
};
use fundamental_runtime::OutboxDrain;
use fundamental_sql::{SqlConfig, SqlPool, SqlSliceStore, SqlTransaction};
use fundamental_sync_server::{
    NoParams, RateLimiter, SliceRegistry, SliceServerSource, slice_sync_router,
};
use fundamental_web::{PageRoute, PolicyChain};
use fundamental_ws::{WsConfig, WsListener, WsServer};
use fundamental_ws_client::WsRemoteSource;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, info, warn};

use crate::BenchError;
use crate::live::models::{BenchJob, Fired, Note, SyncedNote};
use crate::workloads::render;

const NOTES_SUBJECT: &str = "bench.notes";

const NOTES_STREAM: &str = "BENCH_NOTES";

/// The sync readers (`reader-0` …): several, as a server admits bounded connections per user.
const READERS: usize = 8;
const READER_NOTES: i64 = 100;

/// The first number of the readers' notes, above any a write makes.
const READER_FIRST: i64 = 1_000_000_000;

const SECRET: &[u8] = b"fundamental-bench-secret";
const FUTURE: i64 = 4_102_444_800; // 2100-01-01

/// Where the app's engines are.
#[derive(Debug, Clone)]
pub struct Engines {
    /// A `postgresql://` URL the pool connects to.
    pub sql_url: String,
    /// The NATS client's configuration.
    pub nats: NatsConfig,
    /// The most connections the pool opens.
    pub max_connections: u32,
}

#[derive(Clone)]
struct AppState {
    pool: SqlPool,
    jetstream: JetStream,
    /// This boot's name for its jobs: the job table outlives a boot, one row per subject.
    boot: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct NewNote {
    n: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    n: i64,
}

/// Stores the note and schedules its reminder in one transaction, then publishes its event
/// under the note's message id; every step is idempotent, so a retry writes nothing twice.
#[action(POST, "/api/notes")]
async fn add_note(ctx: HttpContext<AppState>, input: NewNote) -> Result<Saved, AnyError> {
    let state = ctx.state().clone();
    let n = input.n;
    state
        .pool
        .transaction(
            IsolationLevel::ReadCommitted,
            async |tx: &mut SqlTransaction| write_note(tx, state.boot, n).await,
        )
        .await?;
    state
        .jetstream
        .publish_bytes_with_msg_id(
            NOTES_SUBJECT,
            Bytes::from(n.to_string()),
            &format!("note-{n}"),
        )
        .await?;
    Ok(Saved { n })
}

mount_actions! { AddNote }

async fn write_note(tx: &mut SqlTransaction, boot: i64, n: i64) -> Result<(), JobError> {
    let note = Note {
        n,
        owner_id: "writer".to_owned(),
        body: format!("Note {n}, written by the load."),
    };
    RepositoryTransaction::insert_if_absent(tx, &note).await?;
    let subject = JobSubject::new(format!("note/{boot}/{n}"))?;
    let scheduled = tx.schedule(BenchJob::Reminder, &subject, due_now()).await?;
    debug!(n, ?scheduled, "bench: the note's reminder is scheduled");
    Ok(())
}

fn due_now() -> chrono::DateTime<Utc> {
    Utc::now() - TimeDelta::seconds(5)
}

struct Firing;

impl JobHandler<BenchJob> for Firing {
    async fn fire(
        &self,
        tx: &mut SqlTransaction,
        job: &FiringJob<BenchJob>,
    ) -> Result<Fire, AnyError> {
        let fired = Fired {
            subject: job.subject.as_str().to_owned(),
        };
        RepositoryTransaction::insert_if_absent(tx, &fired).await?;
        Ok(Fire::Done)
    }
}

/// The app a live run drives, served in this process.
pub struct BenchApp {
    http: reqwest::Client,
    page_url: String,
    action_url: String,
    ws_url: String,
    pool: SqlPool,
    jetstream: JetStream,
    drain: JobsDrain<BenchJob, Firing>,
    readers: Vec<WsRemoteSource>,
    next: AtomicU64,
    boot: i64,
    api: ApiListener,
    ws: WsListener,
}

impl BenchApp {
    /// Builds the schema, starts the HTTP and WebSocket servers, and connects `sync_clients`.
    ///
    /// # Errors
    ///
    /// The first step that failed.
    pub async fn boot(engines: &Engines, sync_clients: usize) -> Result<Self, AnyError> {
        let pool = connect_pool(engines).await?;
        schema(&pool).await?;
        let jetstream = notes_stream(engines).await?;
        let started = Utc::now().timestamp_millis();
        let (api, http_at) = serve_api(AppState {
            pool: pool.clone(),
            jetstream: jetstream.clone(),
            boot: started,
        })
        .await?;
        let (ws, ws_url, readers) = serve_sync(&pool, sync_clients).await?;
        let drain = JobsDrain::new(
            pool.clone(),
            Arc::new(Firing),
            JobsSettings {
                lease_secs: 5,
                backoff_base_secs: 1,
                backoff_max_secs: 2,
                ..JobsSettings::default()
            },
        )?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| boot("build the HTTP client", &error.to_string()))?;
        info!(http = %http_at, ws = %ws_url, sync_clients, "bench: the app is up");
        Ok(Self {
            http,
            page_url: format!("http://{http_at}/"),
            action_url: format!("http://{http_at}/api/notes"),
            ws_url,
            pool,
            jetstream,
            drain,
            readers,
            next: AtomicU64::new(0),
            boot: started,
            api,
            ws,
        })
    }

    /// Asks for the page and reads it whole.
    ///
    /// # Errors
    ///
    /// The transport's error, or the status when it is not 200.
    pub async fn page(&self) -> Result<(), String> {
        let response = self
            .http
            .get(&self.page_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        if status.is_success() && body.contains("Project 49") {
            Ok(())
        } else {
            Err(format!("the page answered {status}"))
        }
    }

    /// Calls the action for write `n`.
    ///
    /// # Errors
    ///
    /// The transport's error, or the status and body when it is not 200.
    pub async fn write(&self, n: u64) -> Result<(), String> {
        let response = self
            .http
            .post(&self.action_url)
            .json(&json!({ "n": n }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(format!("the action answered {status}: {body}"))
    }

    /// Calls the action for the next write of a load, numbered above any fault run's.
    ///
    /// # Errors
    ///
    /// As [`write`](Self::write).
    pub async fn next_write(&self) -> Result<(), String> {
        let n = 1_000_000 + self.next.fetch_add(1, Ordering::Relaxed);
        self.write(n).await
    }

    /// One worker tick: schedules a job due now, then claims and fires one due job.
    ///
    /// # Errors
    ///
    /// The scheduling's or the drain's error.
    pub async fn worker_tick(&self) -> Result<(), String> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let subject =
            JobSubject::new(format!("tick/{}/{n}", self.boot)).map_err(|e| e.to_string())?;
        let scheduled = self
            .pool
            .transaction(
                IsolationLevel::ReadCommitted,
                async |tx: &mut SqlTransaction| {
                    tx.schedule(BenchJob::Reminder, &subject, due_now()).await
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        debug!(n, ?scheduled, "bench: a tick's job is scheduled");
        self.drain.drain(1).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Pulls a reader's slice over the WebSocket, on each connected client in turn.
    ///
    /// # Errors
    ///
    /// The sync error, or a page of the wrong size.
    pub async fn pull(&self) -> Result<(), String> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let reader = self
            .readers
            .get(usize::try_from(n).unwrap_or_default() % self.readers.len().max(1))
            .ok_or("no sync client is connected")?;
        let rows = reader
            .pull_slice("mine", Value::Null, 0)
            .await
            .map_err(|e| e.to_string())?;
        if i64::try_from(rows.len()) == Ok(READER_NOTES) {
            Ok(())
        } else {
            Err(format!("the slice held {} rows", rows.len()))
        }
    }

    /// Every write found once when its row and event are each there once, twice when either
    /// is duplicated, and not at all when either is missing.
    ///
    /// # Errors
    ///
    /// A store that could not be read.
    pub async fn written(&self, writes: u64) -> Result<Vec<u64>, String> {
        let rows = self.rows(writes).await?;
        let events = self.events(writes).await?;
        let mut found = Vec::new();
        for n in 0..writes {
            let times = match (rows.get(&n), events.get(&n)) {
                (Some(1), Some(1)) => 1,
                (Some(_), Some(_)) => 2,
                _ => 0,
            };
            found.extend(std::iter::repeat_n(n, times));
        }
        Ok(found)
    }

    async fn rows(&self, writes: u64) -> Result<BTreeMap<u64, u32>, String> {
        let rows = self
            .pool
            .fetch_all(&format!(
                "SELECT n, count(*)::BIGINT AS times FROM bench_notes WHERE n < {writes} GROUP BY n"
            ))
            .await
            .map_err(|e| e.to_string())?;
        let mut counts = BTreeMap::new();
        for row in rows {
            use fundamental_sql::sqlx::Row;
            let n: i64 = row.try_get("n").map_err(|e| e.to_string())?;
            let times: i64 = row.try_get("times").map_err(|e| e.to_string())?;
            counts.insert(
                u64::try_from(n).unwrap_or_default(),
                u32::try_from(times).unwrap_or(u32::MAX),
            );
        }
        Ok(counts)
    }

    async fn events(&self, writes: u64) -> Result<BTreeMap<u64, u32>, String> {
        let stream = self
            .jetstream
            .get_stream(NOTES_STREAM)
            .await
            .map_err(|e| e.to_string())?;
        let consumer = stream
            .create_consumer(
                ConsumerConfig::new(format!("readback_{}", Utc::now().timestamp_millis()))
                    .deliver_policy(DeliverPolicy::All)
                    .ack_policy(AckPolicy::None),
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut deliveries = consumer.deliveries().await.map_err(|e| e.to_string())?;
        let mut counts = BTreeMap::new();
        loop {
            let Ok(Some(delivery)) =
                tokio::time::timeout(Duration::from_secs(3), deliveries.next()).await
            else {
                break;
            };
            let n = delivery
                .msg_id()
                .and_then(|id| id.strip_prefix("note-"))
                .and_then(|n| n.parse::<u64>().ok());
            match n {
                Some(n) if n < writes => *counts.entry(n).or_default() += 1,
                Some(_) => {}
                None => warn!("bench: an event without a note's message id"),
            }
        }
        Ok(counts)
    }

    /// How many jobs have fired.
    ///
    /// # Errors
    ///
    /// The query's error.
    pub async fn fired(&self) -> Result<u64, String> {
        use fundamental_sql::sqlx::Row;
        let row = self
            .pool
            .fetch_one("SELECT count(*)::BIGINT AS fired FROM bench_fired")
            .await
            .map_err(|e| e.to_string())?;
        let fired: i64 = row.try_get("fired").map_err(|e| e.to_string())?;
        Ok(u64::try_from(fired).unwrap_or_default())
    }

    /// Fires every job that is due, a batch at a time, until none is.
    ///
    /// # Errors
    ///
    /// The drain's error.
    pub async fn drain_all(&self) -> Result<u64, String> {
        let mut fired = 0;
        loop {
            let report = self.drain.drain(100).await.map_err(|e| e.to_string())?;
            if report.claimed == 0 {
                return Ok(fired);
            }
            fired += u64::try_from(report.relayed).unwrap_or_default();
        }
    }

    /// The WebSocket URL sync clients connect to.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Stops both servers.
    ///
    /// # Errors
    ///
    /// The first server that did not drain.
    pub async fn stop(self) -> Result<(), AnyError> {
        self.api.stop().await?;
        self.ws.stop().await?;
        Ok(())
    }
}

async fn connect_pool(engines: &Engines) -> Result<SqlPool, AnyError> {
    let config = SqlConfig::development(&engines.sql_url)
        .max_connections(engines.max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5));
    Ok(SqlPool::connect(&config)
        .await?
        .with_key_manager(sealing()?))
}

async fn notes_stream(engines: &Engines) -> Result<JetStream, AnyError> {
    let jetstream = NatsClient::connect(engines.nats.clone()).await?.jetstream();
    if let Err(error) = jetstream.delete_stream(NOTES_STREAM).await {
        info!(%error, "bench: no earlier notes stream to delete");
    }
    jetstream
        .create_stream(StreamConfig::new(NOTES_STREAM).subjects([NOTES_SUBJECT]))
        .await?;
    Ok(jetstream)
}

async fn serve_api(state: AppState) -> Result<(ApiListener, SocketAddr), AnyError> {
    let page = PageRoute::new("/", PolicyChain::root())?
        .view(render::page_view, |_, body: &str| {
            format!("<!doctype html><html><body>{body}</body></html>")
        });
    let router = register_actions(Router::<AppState>::new())?.route(page)?;
    let api = ApiServer::new(ServerConfig::new(loopback()), state, router)
        .serve()
        .await?;
    let at = api
        .local_addr()
        .address()
        .ok_or_else(|| boot("bind the API server", "it bound no address"))?;
    Ok((api, at))
}

async fn serve_sync(
    pool: &SqlPool,
    clients: usize,
) -> Result<(WsListener, String, Vec<WsRemoteSource>), AnyError> {
    let registry = SliceRegistry::new()
        .define::<SyncedNote, NoParams, _, _>("mine", |ctx, _| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        });
    let source = SliceServerSource::new(registry, SqlSliceStore::new(pool));
    let limiter = Arc::new(RateLimiter::new(1_000_000, 1_000_000.0, 65_536));
    let router = slice_sync_router(Arc::new(verifier()), Arc::new(source), limiter);
    let ws = WsServer::new(WsConfig::new(loopback()), router)
        .serve()
        .await?;
    let url = format!("ws://{}", ws.local_addr());
    let mut readers = Vec::with_capacity(clients);
    for client in 0..clients {
        let reader = WsRemoteSource::connect(&url).await?;
        reader
            .authenticate(&token_for(&format!("reader-{}", client % READERS))?)
            .await?;
        readers.push(reader);
    }
    Ok((ws, url, readers))
}

async fn schema(pool: &SqlPool) -> Result<(), AnyError> {
    pool.execute_ddl("DROP TABLE IF EXISTS bench_notes").await?;
    pool.execute_ddl("DROP TABLE IF EXISTS bench_fired").await?;
    pool.execute_ddl(
        "CREATE TABLE bench_notes (n BIGINT PRIMARY KEY, owner_id TEXT NOT NULL, \
         body TEXT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
         version BIGINT NOT NULL DEFAULT 1, _tombstone BOOL NOT NULL DEFAULT false)",
    )
    .await?;
    // The sync slice's index; without it each pull scans every note.
    pool.execute_ddl(
        "CREATE INDEX bench_notes_by_owner ON bench_notes (owner_id HASH, updated_at ASC, n ASC)",
    )
    .await?;
    pool.execute_ddl("CREATE TABLE bench_fired (subject TEXT PRIMARY KEY)")
        .await?;
    let readers = i64::try_from(READERS).unwrap_or(1);
    pool.execute(&format!(
        "INSERT INTO bench_notes (n, owner_id, body) \
         SELECT g, 'reader-' || ((g - {READER_FIRST}) / {READER_NOTES}), \
                'A note a reader syncs, number ' || g \
         FROM generate_series({READER_FIRST}, {}) AS g",
        READER_FIRST + readers * READER_NOTES - 1
    ))
    .await?;
    let dir = tempfile::tempdir()
        .map_err(|error| boot("make a migrations directory", &error.to_string()))?;
    MigrateRunner::new(
        MigrationConfig {
            migrations_dir: dir.path().to_path_buf(),
            environment: "bench".to_owned(),
            is_protected: false,
        },
        MigrationGuard::new(1_000, Lints::default(), &[]),
    )
    .with_embedded(&JOBS_MIGRATIONS)
    .apply_sql(pool)
    .await?;
    Ok(())
}

fn sealing() -> Result<SharedKeyManager, AnyError> {
    Ok(Arc::new(
        KeyManager::builder()
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[0x51; 32])
            .build()?,
    ))
}

fn loopback() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

fn verifier() -> JwtVerifier {
    JwtVerifier::new(
        DecodingKey::from_secret(SECRET),
        Validation::new(Algorithm::HS256),
    )
}

fn token_for(subject: &str) -> Result<SecretString, AnyError> {
    let token = encode(
        &Header::new(Algorithm::HS256),
        &json!({ "sub": subject, "exp": FUTURE }),
        &EncodingKey::from_secret(SECRET),
    )
    .map_err(|error| boot("sign the sync token", &error.to_string()))?;
    Ok(SecretString::new(token))
}

fn boot(step: &str, cause: &str) -> AnyError {
    AnyError::new(BenchError::Boot {
        step: step.to_owned(),
        cause: cause.to_owned(),
    })
}
