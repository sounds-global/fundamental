//! An outbox claim decodes each row on its own: one row that refuses costs that row, never the relay behind it.

use std::borrow::Cow;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use fundamental_core::AnyError;
use fundamental_core::model::{HasLeadingKey, SqlDb};
use fundamental_core::secrets::{Secret, SecretString};
use fundamental_db::{
    Filter, OutboxClaim, OutboxClaimer, OutboxRow, QueryBuilder, RefusalKind, Repository,
};
use fundamental_encryption::{
    ConfiguredKey, ConfiguredKeySource, EncryptedKeyConfig, EncryptionAlgorithm, EnvelopeKeySource,
    KeyManager, KeyManagerBuilder, KeyResult, KmsProvider, SharedKeyManager, material_fingerprint,
};
use fundamental_macros::Model;
use fundamental_sql::SqlPool;
use fundamental_test_containers::sql::SqlSchema;

#[derive(Clone, Debug, Model)]
#[model(sql, outbox)]
struct QueuedEvent {
    #[model(id)]
    id: uuid::Uuid,
    msg_id: String,
    #[model(encrypted = "pii")]
    payload: SecretString,
    relayed_at: Option<DateTime<Utc>>,
    #[model(unbound)]
    attempts: Option<i64>,
    #[model(unbound)]
    last_error: Option<String>,
    #[model(unbound)]
    next_attempt_at: DateTime<Utc>,
}

impl OutboxRow<SqlDb> for QueuedEvent {
    type Scope = ();

    fn msg_id(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.msg_id)
    }

    fn attempts(&self) -> i64 {
        self.attempts.unwrap_or(0)
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
        self.attempts = Some(self.attempts.unwrap_or(0) + 1);
        self.last_error = Some(error.to_owned());
    }
}

const LEASE: TimeDelta = TimeDelta::seconds(30);

async fn claim_at(
    pool: &SqlPool,
    now: DateTime<Utc>,
    lease_until: DateTime<Utc>,
) -> Result<OutboxClaim<QueuedEvent>, AnyError> {
    OutboxClaimer::<QueuedEvent, SqlDb>::claim(pool, &(), 10, now, lease_until).await
}

const SEAL_MATERIAL: u8 = 0x51;

fn key_manager(key_id: &str, material: u8) -> SharedKeyManager {
    Arc::new(
        KeyManager::builder()
            .with_local_key(key_id, &[material; 32])
            .with_local_key(KeyManager::OUTBOX_SEAL_KEY, &[SEAL_MATERIAL; 32])
            .build()
            .expect("build key manager"),
    )
}

async fn queued(count: usize) -> (SqlSchema, SqlPool, Vec<uuid::Uuid>, DateTime<Utc>) {
    queued_under(count, key_manager("pii", 7)).await
}

async fn declared_key_manager() -> SharedKeyManager {
    let key = |id: &str, purpose: &str, byte: u8| ConfiguredKey {
        id: id.to_owned(),
        purpose: purpose.to_owned(),
        material: Secret::new(format!("{purpose}:1:{}", format!("{byte:02x}").repeat(32))),
        version: 1,
        algorithm: EncryptionAlgorithm::ChaCha20Poly1305,
        fingerprint: Some(
            material_fingerprint(&[byte; 32])
                .iter()
                .map(|part| format!("{part:02x}"))
                .collect(),
        ),
    };
    let source = ConfiguredKeySource::new(vec![
        key("pii", "test.at-rest.v1", 0x07),
        key(
            KeyManager::OUTBOX_SEAL_KEY,
            "test.outbox-seal.v1",
            SEAL_MATERIAL,
        ),
    ]);
    let manager = KeyManagerBuilder::from_source(&source)
        .await
        .expect("declared keys load");
    assert!(manager.fingerprints_declared());
    Arc::new(manager)
}

/// A KMS whose ciphertext is the key itself, standing in for Vault Transit unwrapping a DEK.
struct Unwrapping;

#[async_trait::async_trait]
impl KmsProvider for Unwrapping {
    async fn decrypt(&self, ciphertext: &[u8], _key_id: &str) -> KeyResult<Vec<u8>> {
        Ok(ciphertext.to_vec())
    }

    async fn encrypt(&self, plaintext: &[u8], _key_id: &str) -> KeyResult<Vec<u8>> {
        Ok(plaintext.to_vec())
    }
}

async fn vault_key_manager(declared: bool) -> SharedKeyManager {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let key = |id: &str, material: u8| EncryptedKeyConfig {
        id: id.to_owned(),
        encrypted_dek: STANDARD.encode([material; 32]),
        kms_key_id: format!("transit-{id}"),
        version: 1,
        algorithm: EncryptionAlgorithm::ChaCha20Poly1305,
        fingerprint: declared.then(|| {
            material_fingerprint(&[material; 32])
                .iter()
                .map(|part| format!("{part:02x}"))
                .collect()
        }),
    };
    let source = EnvelopeKeySource::new(
        Box::new(Unwrapping),
        vec![
            key("pii", 0x07),
            key(KeyManager::OUTBOX_SEAL_KEY, SEAL_MATERIAL),
        ],
    );
    let manager = KeyManagerBuilder::from_source(&source)
        .await
        .expect("vault keys load");
    assert_eq!(manager.fingerprints_declared(), declared);
    Arc::new(manager)
}

async fn queued_under(
    count: usize,
    keys: SharedKeyManager,
) -> (SqlSchema, SqlPool, Vec<uuid::Uuid>, DateTime<Utc>) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone().with_key_manager(keys);
    pool.execute_ddl(
        "CREATE TABLE IF NOT EXISTS queued_events \
         (id UUID PRIMARY KEY, msg_id TEXT NOT NULL, payload TEXT NOT NULL, \
          relayed_at TIMESTAMPTZ, attempts BIGINT, last_error TEXT, \
          next_attempt_at TIMESTAMPTZ NOT NULL, outbox_seal BYTEA NOT NULL, \
          outbox_set_aside_at TIMESTAMPTZ, \
          outbox_set_aside_reason TEXT, outbox_set_aside_seal BYTEA, outbox_skip_until TIMESTAMPTZ)",
    )
    .await
    .expect("create queued_events");
    let now = Utc::now();
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = uuid::Uuid::new_v4();
        pool.repository::<QueuedEvent>()
            .insert(&QueuedEvent {
                id,
                msg_id: format!("event-{index}"),
                payload: Secret::new(format!("payload-{index}")),
                relayed_at: None,
                attempts: Some(0),
                last_error: None,
                next_attempt_at: now - TimeDelta::seconds(1),
            })
            .await
            .expect("queue an event");
        ids.push(id);
    }
    (schema, pool, ids, now)
}

async fn marks(
    pool: &SqlPool,
    id: uuid::Uuid,
) -> (Option<DateTime<Utc>>, Option<String>, Option<DateTime<Utc>>) {
    let row = pool
        .fetch_one(&format!(
            "SELECT outbox_set_aside_at, outbox_set_aside_reason, outbox_skip_until \
             FROM queued_events WHERE id = '{id}'"
        ))
        .await
        .expect("read the framework columns");
    (
        sqlx::Row::try_get(&row, "outbox_set_aside_at").expect("set-aside instant"),
        sqlx::Row::try_get(&row, "outbox_set_aside_reason").expect("set-aside reason"),
        sqlx::Row::try_get(&row, "outbox_skip_until").expect("skip instant"),
    )
}

async fn has_set_aside_seal(pool: &SqlPool, id: uuid::Uuid) -> bool {
    let row = pool
        .fetch_one(&format!(
            "SELECT outbox_set_aside_seal IS NOT NULL AS sealed FROM queued_events WHERE id = '{id}'"
        ))
        .await
        .expect("read the set-aside seal");
    sqlx::Row::try_get(&row, "sealed").expect("a boolean")
}

#[tokio::test]
async fn a_row_whose_envelope_is_broken_is_set_aside_and_the_rest_are_claimed() {
    let (_schema, pool, ids, now) = queued(3).await;
    pool.execute(&format!(
        "UPDATE queued_events SET payload = 'not an envelope' WHERE id = '{}'",
        ids[1]
    ))
    .await
    .expect("break one envelope");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("one broken row must not fail the claim");
    assert_eq!(claim.rows.len(), 2, "the two intact rows are claimed");
    assert!(claim.rows.iter().all(|row| row.id != ids[1]));
    assert_eq!(claim.set_aside.count(RefusalKind::EnvelopeRefused), 1);
    assert_eq!(claim.set_aside.total(), 1);
    assert_eq!(claim.skipped, 0);

    let (set_aside_at, reason, skip_until) = marks(&pool, ids[1]).await;
    assert!(
        set_aside_at.is_some(),
        "the refusing row carries when it was set aside"
    );
    assert_eq!(reason.as_deref(), Some("envelope_refused"));
    assert!(skip_until.is_none());
    assert!(
        has_set_aside_seal(&pool, ids[1]).await,
        "a claim seals the set-aside it writes"
    );

    let later = claim_at(&pool, now + LEASE + TimeDelta::seconds(1), now + LEASE * 2)
        .await
        .expect("a later claim");
    assert!(
        later.rows.iter().all(|row| row.id != ids[1]) && later.set_aside.total() == 0,
        "a row set aside is never claimed, or set aside, again"
    );
}

#[tokio::test]
async fn a_value_the_model_cannot_take_is_set_aside() {
    let (_schema, pool, ids, now) = queued(2).await;
    pool.execute(&format!(
        "UPDATE queued_events SET relayed_at = NULL, attempts = NULL WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("clear a column");
    pool.execute_ddl("ALTER TABLE queued_events ALTER COLUMN msg_id DROP NOT NULL")
        .await
        .expect("allow a null message id");
    pool.execute(&format!(
        "UPDATE queued_events SET msg_id = NULL WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("null a required column");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a row with a value its model cannot take must not fail the claim");
    assert_eq!(claim.rows.len(), 1);
    assert_eq!(claim.set_aside.count(RefusalKind::ValueRefused), 1);
    let (_, reason, _) = marks(&pool, ids[0]).await;
    assert_eq!(reason.as_deref(), Some("value_refused"));
}

#[tokio::test]
async fn a_row_sealed_under_a_key_this_replica_does_not_hold_is_skipped_not_set_aside() {
    let (schema, pool, ids, now) = queued(2).await;
    let other_replica = schema
        .pool()
        .clone()
        .with_key_manager(key_manager("other", 9));

    let claim = claim_at(&other_replica, now, now + LEASE)
        .await
        .expect("a replica without the key must not fail the claim");
    assert!(claim.rows.is_empty());
    assert_eq!(claim.skipped, 2);
    assert_eq!(
        claim.set_aside.total(),
        0,
        "a key this process lacks is never the row's fault"
    );
    for id in &ids {
        let (set_aside_at, _, skip_until) = marks(&pool, *id).await;
        assert!(set_aside_at.is_none());
        assert!(
            !has_set_aside_seal(&pool, *id).await,
            "a skip seals nothing"
        );
        assert!(skip_until.is_some(), "a skipped row is left for its lease");
    }

    let during_lease = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a claim during the skip");
    assert!(
        during_lease.rows.is_empty(),
        "a skipped row waits out its lease"
    );

    let after_lease = claim_at(&pool, now + LEASE + TimeDelta::seconds(1), now + LEASE * 2)
        .await
        .expect("a claim after the skip");
    assert_eq!(
        after_lease.rows.len(),
        2,
        "a replica holding the key takes the row once its skip has passed"
    );
}

#[tokio::test]
async fn a_row_whose_binding_was_edited_is_skipped_until_fingerprints_are_declared() {
    let (_schema, pool, ids, now) = queued(2).await;
    pool.execute(&format!(
        "UPDATE queued_events SET msg_id = 'edited' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("edit a bound column");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a row whose binding changed must not fail the claim");
    assert_eq!(claim.rows.len(), 1);
    assert_eq!(claim.skipped, 1);
    assert_eq!(claim.set_aside.total(), 0);
}

#[tokio::test]
async fn a_row_whose_binding_was_edited_is_set_aside_once_fingerprints_are_declared() {
    let (_schema, pool, ids, now) = queued_under(2, declared_key_manager().await).await;
    pool.execute(&format!(
        "UPDATE queued_events SET msg_id = 'edited' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("edit a bound column");

    let claim = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a row whose binding changed must not fail the claim");
    assert_eq!(claim.rows.len(), 1);
    assert_eq!(
        claim.set_aside.count(RefusalKind::DecryptRefused),
        1,
        "under declared keys a value that fails to authenticate is the row's own refusal"
    );
    assert_eq!(claim.skipped, 0);
    let (_, reason, skip_until) = marks(&pool, ids[0]).await;
    assert_eq!(reason.as_deref(), Some("decrypt_refused"));
    assert!(skip_until.is_none());
}

/// The same keys, now declared by fingerprint: a forged row is set aside once instead of skipped on every lease.
#[tokio::test]
async fn a_vault_key_declared_by_fingerprint_sets_a_forged_row_aside_once() {
    let (schema, _before, ids, now) = queued_under(2, vault_key_manager(false).await).await;
    let pool = schema
        .pool()
        .clone()
        .with_key_manager(vault_key_manager(true).await);
    pool.execute(&format!(
        "UPDATE queued_events SET msg_id = 'forged' WHERE id = '{}'",
        ids[0]
    ))
    .await
    .expect("forge a bound column");

    let first = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a forged row must not fail the claim");
    assert_eq!(
        first.rows.len(),
        1,
        "the row sealed before the upgrade still relays"
    );
    assert_eq!(first.rows[0].id, ids[1]);
    assert_eq!(first.set_aside.count(RefusalKind::DecryptRefused), 1);
    assert_eq!(first.skipped, 0, "declared keys leave nothing to skip");

    let later = now + LEASE + TimeDelta::seconds(1);
    let second = claim_at(&pool, later, later + LEASE)
        .await
        .expect("a second claim");
    assert!(
        second.rows.iter().all(|row| row.id != ids[0])
            && second.set_aside.total() == 0
            && second.skipped == 0,
        "the forged row was set aside once, and is not met again"
    );
}

#[tokio::test]
async fn rows_no_replica_can_open_never_delay_a_row_that_decodes() {
    // More rows than one claim takes, all older and all under a payload key this replica lacks.
    let (schema, _writer, _planted, now) = queued_under(11, key_manager("pii", 9)).await;
    let pool = schema
        .pool()
        .clone()
        .with_key_manager(key_manager("pii", 7));
    let first = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a first claim");
    let second = claim_at(&pool, now, now + LEASE)
        .await
        .expect("a second claim");
    assert_eq!(first.skipped + second.skipped, 11);

    let fresh = uuid::Uuid::new_v4();
    pool.repository::<QueuedEvent>()
        .insert(&QueuedEvent {
            id: fresh,
            msg_id: "fresh".to_owned(),
            payload: Secret::new("fresh".to_owned()),
            relayed_at: None,
            attempts: Some(0),
            last_error: None,
            next_attempt_at: now,
        })
        .await
        .expect("queue a row this replica can open");

    let after_lease = claim_at(&pool, now + LEASE + TimeDelta::seconds(1), now + LEASE * 2)
        .await
        .expect("a claim once every skip has passed");
    assert!(
        after_lease.rows.iter().any(|row| row.id == fresh),
        "a row that decodes is claimed ahead of every row a claim skipped, however old those are"
    );
    assert_eq!(
        after_lease.skipped, 9,
        "the skipped rows fill only the room the row that decodes left"
    );
}
