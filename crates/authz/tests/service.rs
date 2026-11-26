//! The standalone service's `#[respond]` handlers and RPC contracts.
#![cfg(all(feature = "service", feature = "offline", not(target_arch = "wasm32")))]
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::sync::Arc;

use fundamental_authz::authorizer::{DynAuthorizer, NativeEngine};
use fundamental_authz::backend::OfflineTupleStore;
use fundamental_authz::schema::{NamespaceName, RelationName, SchemaIr};
use fundamental_authz::service::{
    AuthzService, CheckRpcRequest, WriteRpcRequest, check_rpc, describe_responders, write_rpc,
};
use fundamental_authz::tuple::{CanonicalId, ObjectRef, RelationTuple, SubjectRef};
use fundamental_macros::Namespace;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Document {
    #[relation(direct(User))]
    Owner,
    #[relation(union(Owner, direct(User)))]
    Viewer,
}

fn schema() -> Arc<SchemaIr> {
    Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace::<Document>()
            .build()
            .expect("valid schema"),
    )
}

fn object() -> ObjectRef {
    ObjectRef::new(
        NamespaceName::new("document").unwrap(),
        CanonicalId::new("readme").unwrap(),
    )
}

fn alice() -> SubjectRef {
    SubjectRef::leaf(
        NamespaceName::new("user").unwrap(),
        CanonicalId::new("alice").unwrap(),
    )
}

async fn service() -> AuthzService {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");
    let authz: Arc<dyn DynAuthorizer> = Arc::new(NativeEngine::new(store, schema()));
    authz
        .write(&[RelationTuple::new(
            object(),
            RelationName::new("owner").unwrap(),
            alice(),
        )])
        .await
        .expect("write");
    AuthzService::new(authz)
}

fn check(relation: &str, subject: SubjectRef) -> CheckRpcRequest {
    CheckRpcRequest {
        object: object(),
        relation: RelationName::new(relation).unwrap(),
        subject,
        context: Default::default(),
        consistency: Default::default(),
    }
}

#[tokio::test]
async fn check_rpc_answers_a_granted_relation() {
    let service = service().await;
    let reply = check_rpc(&service, check("viewer", alice())).await;
    let response = reply.outcome.expect("a decision, not an error");
    assert!(response.allowed);
}

#[tokio::test]
async fn check_rpc_answers_a_missing_relation_with_a_deny_not_an_error() {
    let service = service().await;
    let bob = SubjectRef::leaf(
        NamespaceName::new("user").unwrap(),
        CanonicalId::new("bob").unwrap(),
    );
    // A clean deny is a decision (`Ok`), never an error.
    let reply = check_rpc(&service, check("viewer", bob)).await;
    let response = reply.outcome.expect("a decision, not an error");
    assert!(!response.allowed);
}

#[tokio::test]
async fn check_rpc_surfaces_an_infra_error_instead_of_denying() {
    let service = service().await;
    // An undefined relation is an error in the reply, not "not allowed".
    let reply = check_rpc(&service, check("nonexistent", alice())).await;
    let error = reply
        .outcome
        .expect_err("an undefined relation must surface as an error");
    assert_eq!(error.code, "AUTHZ_003");
    assert_eq!(error.status, 500);
}

#[tokio::test]
async fn write_rpc_commits_and_returns_a_zookie() {
    let service = service().await;
    let request = WriteRpcRequest {
        tuples: vec![RelationTuple::new(
            ObjectRef::new(
                NamespaceName::new("document").unwrap(),
                CanonicalId::new("guide").unwrap(),
            ),
            RelationName::new("owner").unwrap(),
            alice(),
        )],
    };
    let reply = write_rpc(&service, request).await;
    let zookie = reply.outcome.expect("write commits");
    let follow_up = check_rpc(
        &service,
        CheckRpcRequest {
            object: ObjectRef::new(
                NamespaceName::new("document").unwrap(),
                CanonicalId::new("guide").unwrap(),
            ),
            relation: RelationName::new("owner").unwrap(),
            subject: alice(),
            context: Default::default(),
            consistency: fundamental_authz::zookie::Consistency::AtLeastAsFresh(zookie),
        },
    )
    .await;
    assert!(follow_up.outcome.expect("decision").allowed);
}

#[tokio::test]
async fn check_rpc_request_round_trips_through_json() {
    let request = check("viewer", alice());
    let bytes = serde_json::to_vec(&request).expect("serialize");
    let back: CheckRpcRequest = serde_json::from_slice(&bytes).expect("deserialize");
    assert_eq!(request, back);
}

#[test]
fn describe_responders_lists_every_rpc() {
    let subjects: Vec<&str> = describe_responders()
        .iter()
        .map(|descriptor| descriptor.subject)
        .collect();
    assert_eq!(
        subjects,
        vec![
            "authz.rpc.check",
            "authz.rpc.expand",
            "authz.rpc.list.objects",
            "authz.rpc.write",
            "authz.rpc.delete",
        ]
    );
}
