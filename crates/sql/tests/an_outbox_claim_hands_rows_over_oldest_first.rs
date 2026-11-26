//! A claim hands its rows over in the order it read them: the oldest next attempt first, which
//! for rows never attempted is the order they were written, and for scheduled rows is due order.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{Filter, OutboxClaimer, OutboxRow, QueryBuilder, Repository};
use fundamental_encryption::{KeyManager, SharedKeyManager};
use fundamental_macros::Model;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct Parcel {
    #[model(id)]
    id: uuid::Uuid,
    label: String,
    delivered_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for Parcel {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Owned(self.id.to_string())
    }

    fn attempts(&self) -> i64 {
        self.attempts
    }

    fn pending(_scope: &Self::Scope) -> QueryBuilder<Self, SqlDb, HasLeadingKey> {
        QueryBuilder::new()
            .filter(Filter::is_null(Self::DELIVERED_AT))
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

fn sealing() -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[0x51; 32])
            .build()
            .expect("an outbox seal key"),
    )
}

#[tokio::test]
async fn a_claim_hands_its_rows_over_oldest_next_attempt_first() {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(sealing());
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS parcels \
         (id UUID PRIMARY KEY, label TEXT NOT NULL, delivered_at TIMESTAMPTZ, \
          attempts BIGINT NOT NULL, last_error TEXT, next_attempt_at TIMESTAMPTZ NOT NULL, \
          outbox_seal BYTEA NOT NULL, outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create parcels");
    let now = Utc::now();
    // Neither insertion order nor key order can pass for due order.
    for minutes in [7_i64, 2, 9, 0, 5, 1, 8, 3, 6, 4, 11, 10] {
        pool.repository::<Parcel>()
            .insert(&Parcel {
                id: uuid::Uuid::new_v4(),
                label: format!("{minutes:02}"),
                delivered_at: None,
                attempts: 0,
                last_error: None,
                next_attempt_at: now - TimeDelta::hours(1) + TimeDelta::minutes(minutes),
            })
            .await
            .expect("queue a parcel");
    }

    let claim =
        OutboxClaimer::<Parcel, SqlDb>::claim(&pool, &(), 50, now, now + TimeDelta::seconds(30))
            .await
            .expect("a claim");

    let labels: Vec<&str> = claim
        .rows
        .iter()
        .map(|parcel| parcel.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11"
        ]
    );
}
