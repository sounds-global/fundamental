//! A row that names a due column waits at its due time however far out that is, so the inventory
//! counts it as deferred beyond the retry policy only once it is due and still pushed past the
//! horizon — the one state no drain writes.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{
    Filter, HeldCount, OutboxRow, OutboxSetAside, QueryBuilder, Repository, RetryPolicy,
};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_macros::Model;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct Reminder {
    #[model(id)]
    id: uuid::Uuid,
    sent_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    due_at: DateTime<Utc>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for Reminder {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Owned(self.id.to_string())
    }

    fn attempts(&self) -> i64 {
        self.attempts
    }

    fn pending(_scope: &Self::Scope) -> QueryBuilder<Self, SqlDb, HasLeadingKey> {
        QueryBuilder::new()
            .filter(Filter::is_null(Self::SENT_AT))
            .already_addressed()
    }

    fn next_attempt_column() -> &'static str {
        Self::NEXT_ATTEMPT_AT
    }

    fn due_column() -> Option<&'static str> {
        Some(Self::DUE_AT)
    }

    fn defer_until(&mut self, at: DateTime<Utc>) {
        self.next_attempt_at = at;
    }

    fn note_failure(&mut self, error: &str) {
        self.attempts += 1;
        self.last_error = Some(error.to_owned());
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

async fn reminders() -> (SqlSchema, SqlPool) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(sealing());
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS reminders \
         (id UUID PRIMARY KEY, sent_at TIMESTAMPTZ, due_at TIMESTAMPTZ NOT NULL, \
          attempts BIGINT NOT NULL, last_error TEXT, next_attempt_at TIMESTAMPTZ NOT NULL, \
          outbox_seal BYTEA NOT NULL, outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create reminders");
    (schema, pool)
}

async fn remind(pool: &SqlPool, due_at: DateTime<Utc>, next_attempt_at: DateTime<Utc>) {
    pool.repository::<Reminder>()
        .insert(&Reminder {
            id: uuid::Uuid::new_v4(),
            sent_at: None,
            due_at,
            attempts: 0,
            last_error: None,
            next_attempt_at,
        })
        .await
        .expect("schedule a reminder");
}

async fn beyond_policy(pool: &SqlPool, now: DateTime<Utc>) -> HeldCount {
    OutboxSetAside::<Reminder, SqlDb>::inventory(pool, &(), now, RetryPolicy::default(), 100)
        .await
        .expect("an inventory")
        .beyond_policy()
}

#[tokio::test]
async fn the_inventory_is_clear_for_a_row_due_next_month() {
    let (_schema, pool) = reminders().await;
    let now = Utc::now();
    let next_month = now + TimeDelta::days(30);
    remind(&pool, next_month, next_month).await;

    assert_eq!(beyond_policy(&pool, now).await, HeldCount::Exact(0));
}

#[tokio::test]
async fn a_due_row_pushed_out_by_hand_is_counted_beyond_policy() {
    let (_schema, pool) = reminders().await;
    let now = Utc::now();
    remind(&pool, now - TimeDelta::hours(1), now + TimeDelta::days(30)).await;

    assert_eq!(
        beyond_policy(&pool, now).await,
        HeldCount::Exact(1),
        "a row already due that waits a month was put there by something other than the drain"
    );
}

#[tokio::test]
async fn a_row_due_later_counts_once_its_due_time_passes_while_it_is_still_pushed_out() {
    let (_schema, pool) = reminders().await;
    let now = Utc::now();
    let tomorrow = now + TimeDelta::days(1);
    remind(&pool, tomorrow, now + TimeDelta::days(30)).await;

    assert_eq!(beyond_policy(&pool, now).await, HeldCount::Exact(0));
    assert_eq!(
        beyond_policy(&pool, tomorrow + TimeDelta::minutes(1)).await,
        HeldCount::Exact(1)
    );
}
