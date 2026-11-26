//! An outbox table is its own inventory: every row a claim would not relay stays in it, counted by reason.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_core::secrets::{Secret, SecretString};
use fundamental_db::{
    Filter, HeldCount, OutboxClaimer, OutboxInventory, OutboxRow, OutboxSetAside, QueryBuilder,
    RefusalKind, Repository, RetryPolicy, SetAsideBucket,
};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_macros::Model;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct HeldEvent {
    #[model(id)]
    id: uuid::Uuid,
    msg_id: String,
    #[model(encrypted = "pii")]
    payload: SecretString,
    relayed_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for HeldEvent {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.msg_id)
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

const LEASE: TimeDelta = TimeDelta::seconds(30);
const CAP: u64 = 100;

fn key_manager(pii_id: &str, material: u8) -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_local_key(pii_id, &[material; 32])
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[0x51; 32])
            .build()
            .expect("build key manager"),
    )
}

async fn queued(count: usize) -> (SqlSchema, SqlPool, Vec<uuid::Uuid>, DateTime<Utc>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema
        .pool()
        .clone()
        .with_key_manager(key_manager("pii", 7));
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS held_events \
         (id UUID PRIMARY KEY, msg_id TEXT NOT NULL, payload TEXT NOT NULL, \
          relayed_at TIMESTAMPTZ, attempts BIGINT NOT NULL, last_error TEXT, \
          next_attempt_at TIMESTAMPTZ NOT NULL, outbox_seal BYTEA NOT NULL, \
          outbox_set_aside_at TIMESTAMPTZ, outbox_set_aside_reason TEXT, \
          outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create held_events");
    let now = Utc::now();
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = uuid::Uuid::new_v4();
        pool.repository::<HeldEvent>()
            .insert(&HeldEvent {
                id,
                msg_id: format!("event-{index}"),
                payload: Secret::new(format!("payload-{index}")),
                relayed_at: None,
                attempts: 0,
                last_error: None,
                next_attempt_at: now - TimeDelta::seconds(1),
            })
            .await
            .expect("queue an event");
        ids.push(id);
    }
    (schema, pool, ids, now)
}

async fn inventory_at(pool: &SqlPool, now: DateTime<Utc>) -> OutboxInventory {
    OutboxSetAside::<HeldEvent, SqlDb>::inventory(pool, &(), now, RetryPolicy::default(), CAP)
        .await
        .expect("an inventory")
}

async fn inventory_under(
    pool: &SqlPool,
    now: DateTime<Utc>,
    policy: RetryPolicy,
) -> OutboxInventory {
    OutboxSetAside::<HeldEvent, SqlDb>::inventory(pool, &(), now, policy, CAP)
        .await
        .expect("an inventory")
}

async fn schedule(pool: &SqlPool, id: uuid::Uuid, at: DateTime<Utc>) {
    pool.execute(&format!(
        "UPDATE held_events SET next_attempt_at = '{}' WHERE id = '{id}'",
        at.to_rfc3339()
    ))
    .await
    .expect("schedule a row");
}

const RECURRENCE: TimeDelta = TimeDelta::hours(3);

fn recurring() -> RetryPolicy {
    RetryPolicy::default().with_recurrence(RECURRENCE.to_std().expect("a positive period"))
}

async fn requeue(pool: &SqlPool, batch: u64) -> u64 {
    OutboxSetAside::<HeldEvent, SqlDb>::requeue(pool, &(), batch)
        .await
        .expect("a re-queue")
}

async fn claim_at(pool: &SqlPool, now: DateTime<Utc>) -> fundamental_db::OutboxClaim<HeldEvent> {
    OutboxClaimer::<HeldEvent, SqlDb>::claim(pool, &(), 10, now, now + LEASE)
        .await
        .expect("a claim")
}

async fn break_envelope(pool: &SqlPool, id: uuid::Uuid) {
    pool.execute(&format!(
        "UPDATE held_events SET payload = 'not an envelope' WHERE id = '{id}'"
    ))
    .await
    .expect("break one envelope");
}

#[tokio::test]
async fn a_row_a_claim_set_aside_is_counted_under_its_reason() {
    let (_schema, pool, ids, now) = queued(3).await;
    break_envelope(&pool, ids[1]).await;
    assert_eq!(claim_at(&pool, now).await.set_aside.total(), 1);

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Reason(RefusalKind::EnvelopeRefused)),
        HeldCount::Exact(1)
    );
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Unexplained),
        HeldCount::Exact(0)
    );
    assert!(!inventory.is_clear(), "a held row is never a clear table");
}

#[tokio::test]
async fn a_set_aside_written_by_hand_is_unexplained() {
    let (_schema, pool, ids, now) = queued(2).await;
    pool.execute(&format!(
        "UPDATE held_events SET outbox_set_aside_at = now(), \
           outbox_set_aside_reason = 'envelope_refused' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("set a row aside by hand");

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Unexplained),
        HeldCount::Exact(1),
        "a set-aside no claim sealed is not counted under the reason it states"
    );
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Reason(RefusalKind::EnvelopeRefused)),
        HeldCount::Exact(0)
    );
}

#[tokio::test]
async fn a_set_aside_whose_reason_was_edited_is_unexplained() {
    let (_schema, pool, ids, now) = queued(2).await;
    break_envelope(&pool, ids[0]).await;
    claim_at(&pool, now).await;
    pool.execute(&format!(
        "UPDATE held_events SET outbox_set_aside_reason = 'value_refused' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("edit a set-aside's reason");

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Unexplained),
        HeldCount::Exact(1)
    );
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Reason(RefusalKind::ValueRefused)),
        HeldCount::Exact(0)
    );
}

#[tokio::test]
async fn a_row_deferred_beyond_the_retry_policy_is_counted() {
    let (_schema, pool, ids, now) = queued(2).await;
    pool.execute(&format!(
        "UPDATE held_events SET next_attempt_at = now() + interval '1 day' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("defer a row past any retry");

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(inventory.beyond_policy(), HeldCount::Exact(1));
    assert!(!inventory.is_clear());
}

#[tokio::test]
async fn a_row_re_scheduled_within_its_declared_recurrence_is_not_beyond_policy() {
    let (_schema, pool, ids, now) = queued(2).await;
    schedule(&pool, ids[0], now + RECURRENCE).await;

    assert_eq!(
        inventory_under(&pool, now, recurring())
            .await
            .beyond_policy(),
        HeldCount::Exact(0),
        "the relay's own schedule is not a deferral something else wrote"
    );
    assert_eq!(
        inventory_under(&pool, now, RetryPolicy::default())
            .await
            .beyond_policy(),
        HeldCount::Exact(1),
        "the same row under a policy that declares no recurrence is beyond it"
    );
}

#[tokio::test]
async fn a_recurrence_written_by_a_replica_whose_clock_runs_ahead_is_not_beyond_policy() {
    let (_schema, pool, ids, now) = queued(2).await;
    schedule(
        &pool,
        ids[0],
        now + RECURRENCE + LEASE + TimeDelta::minutes(1),
    )
    .await;

    assert_eq!(
        inventory_under(&pool, now, recurring())
            .await
            .beyond_policy(),
        HeldCount::Exact(0),
        "a minute of skew between the writing and the reading replica is not a planted deferral"
    );
}

#[tokio::test]
async fn a_row_scheduled_past_its_declared_recurrence_is_beyond_policy() {
    let (_schema, pool, ids, now) = queued(2).await;
    schedule(&pool, ids[0], now + RECURRENCE + TimeDelta::hours(1)).await;

    let inventory = inventory_under(&pool, now, recurring()).await;
    assert_eq!(inventory.beyond_policy(), HeldCount::Exact(1));
    assert!(!inventory.is_clear());
}

#[tokio::test]
async fn a_row_a_claim_skipped_is_counted() {
    let (schema, pool, _ids, now) = queued(2).await;
    let other_replica = schema
        .pool()
        .clone()
        .with_key_manager(key_manager("other", 9));
    assert_eq!(claim_at(&other_replica, now).await.skipped, 2);

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(inventory.skipped(), HeldCount::Exact(2));
    assert!(!inventory.is_clear());
}

#[tokio::test]
async fn a_skip_stamped_beyond_any_lease_is_unexplained() {
    let (schema, pool, ids, now) = queued(2).await;
    pool.execute(&format!(
        "UPDATE held_events SET outbox_skip_until = now() + interval '1 day' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("skip a row by hand");
    let other_replica = schema
        .pool()
        .clone()
        .with_key_manager(key_manager("other", 9));
    assert_eq!(claim_at(&other_replica, now).await.skipped, 1);

    let inventory = inventory_at(&pool, now).await;
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Unexplained),
        HeldCount::Exact(1),
        "a claim skips a row for one lease, so a skip reaching a day out was written by hand"
    );
    assert_eq!(
        inventory.skipped(),
        HeldCount::Exact(1),
        "only the skip a claim wrote counts as skipped"
    );
}

#[tokio::test]
async fn a_table_that_holds_nothing_is_clear() {
    let (_schema, pool, _ids, now) = queued(3).await;
    assert!(inventory_at(&pool, now).await.is_clear());
}

#[tokio::test]
async fn the_inventory_stops_at_its_cap_and_says_so() {
    let (_schema, pool, _ids, now) = queued(3).await;
    pool.execute(
        "UPDATE held_events SET outbox_set_aside_at = now(), \
           outbox_set_aside_reason = 'value_refused'",
    )
    .await
    .expect("set every row aside by hand");

    let inventory =
        OutboxSetAside::<HeldEvent, SqlDb>::inventory(&pool, &(), now, RetryPolicy::default(), 2)
            .await
            .expect("an inventory");
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Unexplained),
        HeldCount::AtLeast(2)
    );
    assert_eq!(
        inventory.set_aside(SetAsideBucket::Reason(RefusalKind::KeyNotHeld)),
        HeldCount::AtLeast(0),
        "a capped read cannot say a bucket it saw nothing in is empty"
    );
}

#[tokio::test]
async fn a_requeued_row_whose_cause_was_repaired_is_claimed_again() {
    let (_schema, pool, ids, now) = queued(2).await;
    let row = pool
        .fetch_one(&format!(
            "SELECT payload FROM held_events WHERE id = '{}'",
            ids[0]
        ))
        .await
        .expect("read the intact payload");
    let intact: String = sqlx::Row::try_get(&row, "payload").expect("the envelope");
    break_envelope(&pool, ids[0]).await;
    assert_eq!(claim_at(&pool, now).await.set_aside.total(), 1);

    pool.execute(&format!(
        "UPDATE held_events SET payload = '{intact}' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("repair the envelope");
    assert_eq!(requeue(&pool, 10).await, 1);
    assert_eq!(requeue(&pool, 10).await, 0, "nothing is re-queued twice");
    assert!(inventory_at(&pool, now).await.is_clear());

    let later = now + LEASE + TimeDelta::seconds(1);
    let claim = claim_at(&pool, later).await;
    assert!(
        claim.rows.iter().any(|row| row.id == ids[0]),
        "the repaired row is claimed and relayed like any other"
    );
}

#[tokio::test]
async fn two_requeues_at_once_clear_each_row_once() {
    let (_schema, pool, _ids, now) = queued(20).await;
    pool.execute(
        "UPDATE held_events SET outbox_set_aside_at = now(), \
           outbox_set_aside_reason = 'value_refused'",
    )
    .await
    .expect("set every row aside by hand");

    let (first, second) = tokio::join!(requeue(&pool, 20), requeue(&pool, 20));
    let (third, fourth) = tokio::join!(requeue(&pool, 20), requeue(&pool, 20));
    assert_eq!(
        first + second + third + fourth,
        20,
        "every set-aside row is re-queued exactly once"
    );
    assert!(inventory_at(&pool, now).await.is_clear());
}
