//! Every outbox row carries a seal the `Model` derive writes and checks, so a planted or edited row is held at the claim.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_db::{Filter, OutboxClaim, OutboxClaimer, OutboxRow, QueryBuilder, Repository};
use fundamental_encryption::{EncryptionAlgorithm, KeyManager, SharedKeyManager};
use fundamental_macros::Model;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct AuditEntry {
    #[model(id)]
    id: uuid::Uuid,
    commitment: String,
    #[model(erasable)]
    subject: Option<String>,
    relayed_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: i64,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for AuditEntry {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.commitment)
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

fn sealing(versions: &[(u32, u8)]) -> SharedKeyManager {
    Arc::new(
        versions
            .iter()
            .fold(KeyManager::builder(), |builder, (version, material)| {
                builder.with_key_version(
                    KeyManager::OUTBOX_SEAL_KEY,
                    *version,
                    &[*material; 32],
                    EncryptionAlgorithm::ChaCha20Poly1305,
                )
            })
            .build()
            .expect("an outbox seal key"),
    )
}

async fn claim_at(
    pool: &SqlPool,
    now: DateTime<Utc>,
    lease_until: DateTime<Utc>,
) -> Result<OutboxClaim<AuditEntry>, AnyError> {
    OutboxClaimer::<AuditEntry, SqlDb>::claim(pool, &(), 10, now, lease_until).await
}

fn entry(id: uuid::Uuid, index: usize, now: DateTime<Utc>) -> AuditEntry {
    AuditEntry {
        id,
        commitment: format!("leaf-{index}"),
        subject: Some(format!("subject-{index}")),
        relayed_at: None,
        attempts: 0,
        last_error: None,
        next_attempt_at: now - TimeDelta::seconds(1),
    }
}

async fn queued(
    count: usize,
    keys: SharedKeyManager,
) -> (SqlSchema, SqlPool, Vec<uuid::Uuid>, DateTime<Utc>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(keys);
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS audit_entries \
         (id UUID PRIMARY KEY, commitment TEXT NOT NULL, subject TEXT, relayed_at TIMESTAMPTZ, \
          attempts BIGINT NOT NULL, last_error TEXT, next_attempt_at TIMESTAMPTZ NOT NULL, \
          outbox_seal BYTEA NOT NULL, outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create audit_entries");
    let now = Utc::now();
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = uuid::Uuid::new_v4();
        pool.repository::<AuditEntry>()
            .insert(&entry(id, index, now))
            .await
            .expect("queue an entry");
        ids.push(id);
    }
    (schema, pool, ids, now)
}

#[tokio::test]
async fn a_commitment_swapped_by_a_writer_is_refused_at_claim() {
    let (_schema, pool, ids, now) = queued(3, sealing(&[(1, 0x51)])).await;
    pool.execute(&format!(
        "UPDATE audit_entries SET commitment = 'forged-leaf' WHERE id = '{}'",
        ids[1]
    ))
    .await
    .expect("swap a pending commitment");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a swapped row must not fail the claim");
    assert_eq!(claim.rows.len(), 2, "the two untouched rows are claimed");
    assert!(
        claim.rows.iter().all(|row| row.commitment != "forged-leaf"),
        "a swapped commitment is never handed to a relay"
    );
    assert_eq!(claim.skipped, 1, "the swapped row is held at the claim");
    assert_eq!(claim.set_aside.total(), 0);
}

#[tokio::test]
async fn a_seal_copied_from_another_row_does_not_vouch_for_this_one() {
    let (_schema, pool, ids, now) = queued(2, sealing(&[(1, 0x51)])).await;
    pool.execute(&format!(
        "UPDATE audit_entries SET \
           commitment = (SELECT commitment FROM audit_entries WHERE id = '{source}'), \
           outbox_seal = (SELECT outbox_seal FROM audit_entries WHERE id = '{source}') \
         WHERE id = '{target}'",
        source = ids[0],
        target = ids[1],
    ))
    .await
    .expect("copy one row's commitment and seal onto another");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a copied seal must not fail the claim");
    assert_eq!(claim.rows.len(), 1);
    assert_eq!(
        claim.rows[0].id, ids[0],
        "only the row the seal was made for"
    );
    assert_eq!(claim.skipped, 1);
}

#[tokio::test]
async fn a_row_planted_without_the_key_is_refused_at_claim() {
    let (_schema, pool, _ids, now) = queued(1, sealing(&[(1, 0x51)])).await;
    pool.execute(&format!(
        "INSERT INTO audit_entries \
           (id, commitment, subject, attempts, next_attempt_at, outbox_seal) \
         VALUES ('{}', 'planted-leaf', NULL, 0, '{}', decode('00000001' || repeat('00', 32), 'hex'))",
        uuid::Uuid::new_v4(),
        (now - TimeDelta::seconds(1)).to_rfc3339(),
    ))
    .await
    .expect("plant a row with a forged seal");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a planted row must not fail the claim");
    assert_eq!(claim.rows.len(), 1, "the row the service wrote is claimed");
    assert!(
        claim
            .rows
            .iter()
            .all(|row| row.commitment != "planted-leaf")
    );
    assert_eq!(claim.skipped, 1);
}

#[tokio::test]
async fn an_erased_subject_leaves_the_row_claimable() {
    let (_schema, pool, ids, now) = queued(2, sealing(&[(1, 0x51)])).await;
    pool.execute(&format!(
        "UPDATE audit_entries SET subject = NULL WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("erase a subject");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a claim after an erasure");
    assert_eq!(
        claim.rows.len(),
        2,
        "an erasable column is outside the seal, so an erased row still relays"
    );
    assert_eq!(claim.skipped, 0);
}

#[tokio::test]
async fn a_whole_row_update_through_the_model_reseals_the_row() {
    let (_schema, pool, ids, now) = queued(1, sealing(&[(1, 0x51)])).await;
    let mut updated = entry(ids[0], 0, now);
    updated.commitment = "leaf-0-restated".to_owned();
    pool.repository::<AuditEntry>()
        .update(&updated)
        .await
        .expect("update the row through its model");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a claim after the update");
    assert_eq!(claim.rows.len(), 1, "the model re-sealed what it wrote");
    assert_eq!(claim.rows[0].commitment, "leaf-0-restated");
    assert_eq!(claim.skipped, 0);
}

#[tokio::test]
async fn a_row_sealed_under_a_version_this_replica_does_not_hold_is_skipped_not_set_aside() {
    let (schema, pool, _ids, now) = queued(2, sealing(&[(1, 0x51), (2, 0x52)])).await;
    let lagging = schema
        .pool()
        .clone()
        .with_key_manager(sealing(&[(1, 0x51)]));

    let claim = claim_at(&lagging, now, now + LEASE)
        .await
        .expect("a replica that has not rolled the rotation must not fail the claim");
    assert!(claim.rows.is_empty());
    assert_eq!(claim.skipped, 2);
    assert_eq!(
        claim.set_aside.total(),
        0,
        "a key version this process lacks is never the row's fault"
    );

    let rotated = claim_at(&pool, now + LEASE + TimeDelta::seconds(1), now + LEASE * 2)
        .await
        .expect("a replica holding the current version");
    assert_eq!(
        rotated.rows.len(),
        2,
        "a replica holding the version takes the rows once the skip has passed"
    );
}
