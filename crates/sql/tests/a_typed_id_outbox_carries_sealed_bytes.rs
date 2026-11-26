//! An outbox row keyed by a typed id, carrying its body as sealed bytes.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{
    Filter, KeyedRepository, OutboxClaim, OutboxClaimer, OutboxRow, QueryBuilder, Relay,
    Repository, RetryPolicy, drain_pending,
};
use fundamental_encryption::{EncryptionAlgorithm, KeyManager, SharedKeyManager};
use fundamental_macros::{Error, Identifier, Model};
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;
use tracing_test::traced_test;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Identifier)]
struct EventId(uuid::Uuid);

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct EventEntry {
    #[model(id)]
    id: EventId,
    body: Vec<u8>,
    relayed_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for EventEntry {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Owned(self.id.to_string())
    }

    fn attempts(&self) -> i64 {
        self.attempts
    }

    fn pending(_scope: &Self::Scope) -> QueryBuilder<Self, SqlDb, HasLeadingKey> {
        QueryBuilder::new()
            .filter(Filter::is_null(Self::RELAYED_AT))
            .already_addressed()
    }

    fn next_attempt_column() -> &'static str {
        Self::NEXT_ATTEMPT_AT
    }

    fn defer_until(&mut self, at: DateTime<Utc>) {
        self.next_attempt_at = at;
    }

    fn note_failure(&mut self, error: &str) {
        self.attempts += 1;
        self.last_error = Some(error.to_owned());
    }
}

#[derive(Debug, Error)]
enum Unreachable {
    #[error(
        code = "TEST_EVT_001",
        message = "the broker is unreachable",
        status = 503
    )]
    Broker,
}

struct Refusing;

impl Relay<EventEntry> for Refusing {
    async fn relay(&self, _row: &EventEntry) -> Result<(), AnyError> {
        Err(AnyError::new(Unreachable::Broker))
    }
}

/// A body no UTF-8 decoder accepts, so a text column could not have held it.
const BODY: &[u8] = &[0x00, 0xff, 0xfe, b'{', 0x80];

const LEASE: TimeDelta = TimeDelta::seconds(30);

fn sealing() -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_key_version(
                KeyManager::OUTBOX_SEAL_KEY,
                1,
                &[0x5a; 32],
                EncryptionAlgorithm::ChaCha20Poly1305,
            )
            .build()
            .expect("an outbox seal key"),
    )
}

async fn claim_at(pool: &SqlPool, now: DateTime<Utc>) -> OutboxClaim<EventEntry> {
    OutboxClaimer::<EventEntry, SqlDb>::claim(pool, &(), 10, now, now + LEASE)
        .await
        .expect("the claim runs")
}

async fn queued() -> (SqlSchema, SqlPool, [EventId; 2], DateTime<Utc>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(sealing());
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS event_entries \
         (id UUID PRIMARY KEY, body BYTEA NOT NULL, relayed_at TIMESTAMPTZ, \
          attempts BIGINT NOT NULL, last_error TEXT, next_attempt_at TIMESTAMPTZ NOT NULL, \
          outbox_seal BYTEA NOT NULL, outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create event_entries");
    let now = Utc::now();
    let ids = [EventId(uuid::Uuid::new_v4()), EventId(uuid::Uuid::new_v4())];
    for id in ids {
        pool.repository::<EventEntry>()
            .insert(&EventEntry {
                id,
                body: BODY.to_vec(),
                relayed_at: None,
                attempts: 0,
                last_error: None,
                next_attempt_at: now - TimeDelta::seconds(1),
            })
            .await
            .expect("queue an event");
    }
    (schema, pool, ids, now)
}

#[tokio::test]
async fn a_body_that_is_not_text_is_claimed_exactly_as_written() {
    let (_schema, pool, ids, now) = queued().await;

    let claim = claim_at(&pool, now).await;

    assert_eq!(claim.rows.len(), 2);
    assert_eq!(claim.skipped, 0);
    for row in &claim.rows {
        assert_eq!(row.body, BODY, "the sealed body round-trips byte for byte");
        assert!(ids.contains(&row.id));
        assert_eq!(
            row.msg_id(),
            row.id.to_string(),
            "the message id is the id itself"
        );
    }
}

#[tokio::test]
async fn a_body_byte_changed_by_a_writer_is_held_at_the_claim() {
    let (_schema, pool, ids, now) = queued().await;
    pool.execute(&format!(
        "UPDATE event_entries SET body = '\\x00fffe7b81'::bytea WHERE id = '{}'",
        ids[0].0
    ))
    .await
    .expect("change one byte of a pending body");

    let claim = claim_at(&pool, now).await;

    assert_eq!(claim.rows.len(), 1, "only the untouched event is claimed");
    assert_eq!(claim.rows[0].id, ids[1]);
    assert_eq!(claim.skipped, 1, "the changed body is held, never relayed");
}

#[tokio::test]
#[traced_test]
async fn a_failed_relay_is_recorded_and_logged_with_its_code() {
    let (_schema, pool, ids, now) = queued().await;
    let repository = pool.repository::<EventEntry>();
    let policy = RetryPolicy::new(
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(60),
    );

    let report = drain_pending(&pool, &Refusing, &(), 10, policy, now)
        .await
        .expect("the claim runs");

    assert_eq!(report.claimed, 2);
    assert_eq!(report.relayed, 0);
    for id in ids {
        let row = repository
            .find(&id)
            .await
            .expect("read the event back")
            .expect("the event is still queued");
        assert_eq!(row.attempts, 1);
        let recorded = row.last_error.expect("the failure is recorded on the row");
        assert!(
            recorded.starts_with("[TEST_EVT_001] "),
            "the row names its failure by code: {recorded}"
        );
        assert!(row.next_attempt_at > now, "the failed event is deferred");
    }
    assert!(logs_contain("outbox relay failed"));
    assert!(logs_contain("TEST_EVT_001"));
}
