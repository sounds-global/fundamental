//! A failed relay's failure is written onto its row only while the row is still the one its claim leased.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{
    Filter, KeyedRepository, OutboxRow, QueryBuilder, Relay, Repository, RetryPolicy, drain_pending,
};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_macros::{Error, Model};
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;
use tracing_test::traced_test;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct Notice {
    #[model(id)]
    id: uuid::Uuid,
    body: String,
    relayed_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for Notice {
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
enum Lost {
    #[error(
        code = "TEST_NOTICE_001",
        message = "the answer never arrived",
        status = 503
    )]
    Acknowledgement,
}

#[derive(Clone, Copy)]
enum Meanwhile {
    /// Nothing: an honest failure.
    Nothing,
    /// Marks the row relayed and commits, as a relay whose acknowledgement was lost does.
    Relays,
    /// Moves the row's next attempt, as a reschedule or a second worker's claim does.
    Moves(DateTime<Utc>),
}

struct Failing {
    pool: SqlPool,
    meanwhile: Meanwhile,
}

impl Relay<Notice> for Failing {
    async fn relay(&self, row: &Notice) -> Result<(), AnyError> {
        match self.meanwhile {
            Meanwhile::Nothing => {}
            Meanwhile::Relays => {
                let mut relayed = row.clone();
                relayed.relayed_at = Some(Utc::now());
                self.pool
                    .repository::<Notice>()
                    .update(&relayed)
                    .await
                    .map_err(AnyError::new)?;
            }
            Meanwhile::Moves(to) => {
                self.pool
                    .execute(&format!(
                        "UPDATE notices SET next_attempt_at = '{}' WHERE id = '{}'",
                        to.to_rfc3339(),
                        row.id
                    ))
                    .await
                    .map_err(AnyError::new)?;
            }
        }
        Err(AnyError::new(Lost::Acknowledgement))
    }
}

fn sealing() -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[0x51; 32])
            .build()
            .expect("an outbox seal key"),
    )
}

fn policy() -> RetryPolicy {
    RetryPolicy::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(60),
    )
}

async fn one_due_notice() -> (SqlSchema, SqlPool, uuid::Uuid, DateTime<Utc>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(sealing());
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS notices \
         (id UUID PRIMARY KEY, body TEXT NOT NULL, relayed_at TIMESTAMPTZ, \
          attempts BIGINT NOT NULL, last_error TEXT, next_attempt_at TIMESTAMPTZ NOT NULL, \
          outbox_seal BYTEA NOT NULL, outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create notices");
    let now = Utc::now();
    let id = uuid::Uuid::new_v4();
    pool.repository::<Notice>()
        .insert(&Notice {
            id,
            body: "your hold lapses tomorrow".to_owned(),
            relayed_at: None,
            attempts: 0,
            last_error: None,
            next_attempt_at: now - TimeDelta::seconds(1),
        })
        .await
        .expect("queue a notice");
    (schema, pool, id, now)
}

async fn drain(pool: &SqlPool, meanwhile: Meanwhile, now: DateTime<Utc>) {
    let relay = Failing {
        pool: pool.clone(),
        meanwhile,
    };
    let report = drain_pending(pool, &relay, &(), 10, policy(), now)
        .await
        .expect("the claim runs");
    assert_eq!(report.claimed, 1);
    assert_eq!(report.relayed, 0, "the relay reported a failure");
}

async fn stored(pool: &SqlPool, id: uuid::Uuid) -> Notice {
    pool.repository::<Notice>()
        .find(&id)
        .await
        .expect("read the notice")
        .expect("the notice is still in its table")
}

#[tokio::test]
#[traced_test]
async fn a_relay_that_committed_then_errored_is_never_unmarked() {
    let (_schema, pool, id, now) = one_due_notice().await;

    drain(&pool, Meanwhile::Relays, now).await;

    let notice = stored(&pool, id).await;
    assert!(
        notice.relayed_at.is_some(),
        "the relayed mark survives the failure reported after it committed"
    );
    assert_eq!(
        notice.attempts, 0,
        "no failure is written over a relayed row"
    );
    assert_eq!(notice.last_error, None);
    assert!(logs_contain("moved on while it was relayed"));
}

#[tokio::test]
#[traced_test]
async fn a_failed_relay_cannot_undo_a_reschedule_made_while_it_ran() {
    let (_schema, pool, id, now) = one_due_notice().await;
    let rescheduled = DateTime::<Utc>::from_timestamp(now.timestamp() + 86_400, 0)
        .expect("a representable instant");

    drain(&pool, Meanwhile::Moves(rescheduled), now).await;

    let notice = stored(&pool, id).await;
    assert_eq!(
        notice.next_attempt_at, rescheduled,
        "the reschedule stands; the claimed snapshot is not written back over it"
    );
    assert_eq!(notice.attempts, 0);
    assert!(logs_contain("moved on while it was relayed"));
}

#[tokio::test]
async fn a_failed_relay_is_recorded_when_the_row_is_still_its_own() {
    let (_schema, pool, id, now) = one_due_notice().await;

    drain(&pool, Meanwhile::Nothing, now).await;

    let notice = stored(&pool, id).await;
    assert_eq!(notice.relayed_at, None);
    assert_eq!(notice.attempts, 1);
    let recorded = notice.last_error.expect("the failure is recorded");
    assert!(
        recorded.starts_with("[TEST_NOTICE_001] "),
        "recorded by code: {recorded}"
    );
    assert!(
        notice.next_attempt_at > now && notice.next_attempt_at <= now + TimeDelta::seconds(5),
        "deferred by the first backoff, not left at the lease: {}",
        notice.next_attempt_at
    );
}
