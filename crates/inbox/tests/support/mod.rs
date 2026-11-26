//! A schema with the inbox tables and an effect table, a JetStream stream and inbox-safe
//! consumer per test, and the handlers the suites apply.

// Each suite compiles this module into its own binary and uses a different part of it.
#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::time::Duration;

use fundamental_core::model::SqlDb;
use fundamental_db::migration::{
    EmbeddedMigration, EmbeddedMigrations, Lints, MigrationConfig, MigrationGuard, ModelSchema,
    TableSchema,
};
use fundamental_db::{DatabaseError, IsolationLevel, RepositoryTransaction as _};
use fundamental_db_migrator::MigrateRunner;
use fundamental_inbox::{
    Binding, EffectTransaction, INBOX_MIGRATIONS, Inbound, Inbox, InboxConfig, InboxSettings,
    StepFuture,
};
use fundamental_macros::{Error, Model};
use fundamental_nats::{
    AckPolicy, ConsumerConfig, DeliverPolicy, EventId, JetStream, Message, NatsClient, StreamConfig,
};
use fundamental_sql::SqlPool;
use fundamental_test_containers::container_name::NATS_01_ADAPTER;
use fundamental_test_containers::nats::NatsServer;
use fundamental_test_containers::sql::SqlSchema;
use serde::{Deserialize, Serialize};
use sqlx::Row as _;
use uuid::Uuid;

/// The event every suite applies. Each test publishes on a lane of its own, because two
/// streams may not bind one subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Message)]
#[subject_fn = "lane_subject"]
pub struct DepositPaid {
    #[event_id]
    pub id: EventId,
    pub lane: String,
    pub cents: i64,
}

impl DepositPaid {
    pub fn on(lane: &str, cents: i64) -> Self {
        Self {
            id: EventId::generate(),
            lane: lane.to_owned(),
            cents,
        }
    }

    fn lane_subject(&self) -> String {
        subject(&self.lane)
    }
}

pub fn subject(lane: &str) -> String {
    format!("inbox.{lane}")
}

/// The effect: one booking per applied deposit, in the app's own table.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(sql)]
pub struct Booking {
    #[model(id)]
    pub booking_id: Uuid,
    pub event_id: Uuid,
    pub cents: i64,
}

/// What an effect fails with.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BookingError {
    #[error(
        code = "BOOK_001",
        message = "Booking store failed",
        status = 503,
        retryable
    )]
    Store(#[from] DatabaseError),
    #[error(code = "BOOK_002", message = "Booking refused", status = 422)]
    Refused,
    #[error(
        code = "BOOK_003",
        message = "Booking briefly unavailable",
        status = 503,
        retryable
    )]
    Briefly,
}

/// A fresh schema with the inbox tables applied through the embedded migrations, and the
/// effect's table beside them.
pub async fn schema_with_inbox() -> SqlSchema {
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
    .with_embedded(&INBOX_MIGRATIONS)
    .with_embedded(&BOOKINGS)
    .apply_sql(schema.pool())
    .await
    .expect("the inbox and bookings tables apply");
    schema
}

static BOOKING_MIGRATIONS: [EmbeddedMigration; 1] = [EmbeddedMigration::new(
    "0001_bookings",
    "CREATE TABLE IF NOT EXISTS bookings (booking_id UUID NOT NULL, event_id UUID NOT NULL, \
     cents BIGINT NOT NULL, PRIMARY KEY (booking_id HASH))",
)];

static BOOKING_TABLES: [fn() -> TableSchema; 1] = [<Booking as ModelSchema<SqlDb>>::table_schema];

/// The effect's table, applied by the same migrator as the inbox's.
static BOOKINGS: EmbeddedMigrations =
    EmbeddedMigrations::sql("inbox_tests", &BOOKING_MIGRATIONS, &BOOKING_TABLES);

/// Settings short enough for a test: one-second waits, three attempts.
pub fn quick() -> InboxConfig {
    InboxConfig::new(InboxSettings {
        max_attempts: 3,
        effect_deadline_secs: 5,
        retry_cap_secs: 1,
        failed_delay_secs: 1,
        ..InboxSettings::default()
    })
    .expect("valid settings")
}

pub async fn inbox(pool: &SqlPool) -> Inbox {
    Inbox::open(pool.clone(), quick())
        .await
        .expect("the inbox opens")
}

pub async fn nats() -> NatsClient {
    NatsServer::shared(NATS_01_ADAPTER)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

/// A stream on `lane`'s subject with a consumer the inbox accepts: explicit acks, no delivery
/// limit, a two-second ack wait. `duplicate_window` short, so a test can store two copies of
/// one event by waiting it out.
pub async fn stream(js: &JetStream, lane: &str) -> &'static str {
    let name = NatsServer::stream_name(&lane.to_uppercase().replace('-', "_")).expect("a name");
    js.create_stream(
        StreamConfig::new(&name)
            .subjects([subject(lane)])
            .duplicate_window(Duration::from_millis(100)),
    )
    .await
    .expect("create stream");
    consumer(js, &name, lane, DeliverPolicy::All).await;
    Box::leak(name.into_boxed_str())
}

pub async fn consumer(js: &JetStream, stream: &str, name: &str, deliver: DeliverPolicy) {
    js.get_stream(stream)
        .await
        .expect("stream")
        .create_consumer(
            ConsumerConfig::new(name)
                .durable()
                .ack_policy(AckPolicy::Explicit)
                .deliver_policy(deliver)
                .ack_wait(Duration::from_secs(2)),
        )
        .await
        .expect("create consumer");
}

/// The handler every suite's listeners claim their effect for.
pub const HANDLER: &str = "inbox_tests::book";

pub fn binding(effect: &'static str, stream: &'static str, consumer: &'static str) -> Binding {
    Binding::new(
        effect,
        HANDLER,
        stream,
        consumer,
        IsolationLevel::Serializable,
    )
    .expect("a binding")
}

/// Publishes `event` twice as two stored copies with one id — the broker's duplicate window
/// waited out between them, as a relay retrying after a restart would.
pub async fn publish_twice(js: &JetStream, event: &DepositPaid) {
    js.publish_event(event).await.expect("publish");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let again = js.publish_event(event).await.expect("publish again");
    assert!(!again.duplicate, "the broker deduplicated the second copy");
}

/// Books the deposit — the handler as an app writes it.
pub async fn book(
    tx: &mut EffectTransaction<'_>,
    event: &Inbound<DepositPaid>,
) -> Result<(), BookingError> {
    tx.insert(&Booking {
        booking_id: Uuid::now_v7(),
        event_id: event.event_id().as_uuid(),
        cents: event.payload().cents,
    })
    .await?;
    Ok(())
}

/// [`book`] as the inbox calls it — what `#[subscribe(.., inbox = "…")]` generates.
pub fn book_step<'a, 't>(
    tx: &'a mut EffectTransaction<'t>,
    event: &'a Inbound<DepositPaid>,
) -> StepFuture<'a, BookingError> {
    Box::pin(book(tx, event))
}

/// Polls `check` every 100 ms until it holds or `within` passes.
pub async fn eventually<F, Fut>(within: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if check().await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn count(pool: &SqlPool, sql: &str) -> i64 {
    pool.fetch_one(sql)
        .await
        .expect("a count")
        .try_get(0)
        .expect("an integer")
}

pub fn code_of(error: &impl fundamental_core::Error) -> &'static str {
    error.error_code()
}

/// Whether `consumer` on `stream` holds nothing undelivered and nothing unacknowledged.
pub async fn settled(js: &JetStream, stream: &str, consumer: &str) -> bool {
    let Ok(stream) = js.get_stream(stream).await else {
        return false;
    };
    let Ok(mut consumer) = stream.get_consumer(consumer).await else {
        return false;
    };
    consumer
        .info()
        .await
        .is_ok_and(|info| info.num_pending == 0 && info.num_ack_pending == 0)
}
