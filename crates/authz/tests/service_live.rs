//! The standalone service against a real NATS server; needs Docker.
#![cfg(all(feature = "service", feature = "offline", not(target_arch = "wasm32")))]
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::sync::Arc;
use std::time::Duration;

use fundamental_authz::authorizer::{DynAuthorizer, NativeEngine};
use fundamental_authz::backend::OfflineTupleStore;
use fundamental_authz::schema::{NamespaceName, RelationName, SchemaIr};
use fundamental_authz::service::{AuthzService, CheckRpcReply, CheckRpcRequest};
use fundamental_authz::tuple::{CanonicalId, ObjectRef, RelationTuple, SubjectRef};
use fundamental_macros::Namespace;
use fundamental_nats::{NatsClient, NatsError};
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_test_containers::container_name::NATS_04_AUTHZ;
use fundamental_test_containers::nats::NatsServer;

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Document {
    #[relation(direct(User))]
    Owner,
    #[relation(union(Owner, direct(User)))]
    Viewer,
}

async fn client() -> NatsClient {
    NatsServer::shared(NATS_04_AUTHZ)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

async fn authz_service() -> AuthzService {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");
    let schema = Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace::<Document>()
            .build()
            .expect("valid schema"),
    );
    let authz: Arc<dyn DynAuthorizer> = Arc::new(NativeEngine::new(store, schema));
    authz
        .write(&[RelationTuple::new(
            ObjectRef::new(
                NamespaceName::new("document").unwrap(),
                CanonicalId::new("readme").unwrap(),
            ),
            RelationName::new("owner").unwrap(),
            SubjectRef::leaf(
                NamespaceName::new("user").unwrap(),
                CanonicalId::new("alice").unwrap(),
            ),
        )])
        .await
        .expect("seed grant");
    AuthzService::new(authz)
}

#[tokio::test]
async fn check_is_answered_over_the_wire() {
    let service = Arc::new(authz_service().await);
    let responder = Arc::new(client().await);
    let _listeners = Arc::clone(&service)
        .serve(Arc::clone(&responder))
        .await
        .expect("serve");
    responder
        .sync()
        .await
        .expect("the responders are registered");

    let requester = client().await;
    let request = CheckRpcRequest {
        object: ObjectRef::new(
            NamespaceName::new("document").unwrap(),
            CanonicalId::new("readme").unwrap(),
        ),
        relation: RelationName::new("viewer").unwrap(),
        subject: SubjectRef::leaf(
            NamespaceName::new("user").unwrap(),
            CanonicalId::new("alice").unwrap(),
        ),
        context: Default::default(),
        consistency: Default::default(),
    };

    // The responder's interest can lag its flush, so retry until the request routes.
    let reply: CheckRpcReply = 'reply: {
        for _ in 0..40 {
            match requester
                .request::<CheckRpcRequest, CheckRpcReply>(request.clone())
                .await
            {
                Ok(message) => break 'reply message.payload,
                Err(NatsError::NoResponders { .. }) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => panic!("request failed: {e}"),
            }
        }
        panic!("no responder ever answered");
    };

    let response = reply.outcome.expect("a decision, not an error");
    assert!(response.allowed);
}
