//! [`SqlEnrollmentStore`]: an enrollment one instance writes, another verifies. Needs Docker.
#![cfg(feature = "sql")]

use fundamental_auth::UserId;
use fundamental_keys::{DeviceId, KeyError, SigningKey};
use fundamental_sql::SqlPool;
use fundamental_sync_server::{EnrollmentRegistry, SqlEnrollmentStore};
use fundamental_test_containers::sql::SqlSchema;

/// A registry on a fresh store over `pool`: a distinct node.
fn node(pool: &SqlPool) -> EnrollmentRegistry {
    EnrollmentRegistry::with_store(Box::new(SqlEnrollmentStore::new(pool)))
}

#[tokio::test]
async fn enrollment_is_shared_across_server_instances() {
    // Its own schema: the registry scans the whole table.
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    SqlEnrollmentStore::new(&pool)
        .ensure_table()
        .await
        .expect("create table");

    let alice = UserId::new("alice");
    let device = DeviceId::new("device-1");
    let key = SigningKey::generate().expect("key");

    node(&pool)
        .enroll(alice.clone(), device.clone(), key.verifying_key(), 0)
        .await
        .expect("enroll on node A");

    let node_b = node(&pool);
    let sig = key.sign(b"canonical");
    node_b
        .verify_push(&device, &alice, b"canonical", &sig, 1)
        .await
        .expect("node B verifies the push enrolled on node A");
    assert_eq!(
        node_b.owner(&device).await.expect("owner"),
        Some(alice.clone()),
    );

    let new_key = SigningKey::generate().expect("key");
    node_b
        .rotate(&device, new_key.verifying_key(), 10, 100)
        .await
        .expect("rotate on node B");
    let new_sig = new_key.sign(b"canonical");
    node(&pool)
        .verify_push(&device, &alice, b"canonical", &sig, 50)
        .await
        .expect("old key during grace");
    node(&pool)
        .verify_push(&device, &alice, b"canonical", &new_sig, 50)
        .await
        .expect("new key active");
    assert!(matches!(
        node(&pool)
            .verify_push(&device, &alice, b"canonical", &sig, 200)
            .await,
        Err(KeyError::BadSignature)
    ));

    node(&pool).revoke(&device, 300).await.expect("revoke");
    assert!(matches!(
        node(&pool)
            .verify_push(&device, &alice, b"canonical", &new_sig, 301)
            .await,
        Err(KeyError::BadSignature)
    ));
}
