//! The check engine over the real offline SQLite tuple store.
#![cfg(all(feature = "offline", not(target_arch = "wasm32")))]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::sync::Arc;

use fundamental_authz::backend::OfflineTupleStore;
use fundamental_authz::engine::Engine;
use fundamental_authz::schema::{
    NamespaceIr, NamespaceName, RelatedType, RelationIr, RelationName, RewriteIr, SchemaIr,
};
use fundamental_authz::tuple::{
    CanonicalId, CaveatContext, ObjectRef, RelationTuple, SubjectRef, TupleStore,
};
use fundamental_authz::zookie::Consistency;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};

fn nn(s: &str) -> NamespaceName {
    NamespaceName::new(s).expect("namespace")
}
fn rn(s: &str) -> RelationName {
    RelationName::new(s).expect("relation")
}
fn obj(ns: &str, id: &str) -> ObjectRef {
    ObjectRef::new(nn(ns), CanonicalId::new(id).expect("id"))
}

/// document{ parent: [folder], viewer: parent->viewer } · folder{ viewer: [user] }
fn schema() -> Arc<SchemaIr> {
    Arc::new(
        SchemaIr::builder()
            .namespace_ir(NamespaceIr::new(nn("user"), vec![]))
            .namespace_ir(NamespaceIr::new(
                nn("folder"),
                vec![RelationIr::direct(
                    rn("viewer"),
                    vec![RelatedType::leaf(nn("user"))],
                )],
            ))
            .namespace_ir(NamespaceIr::new(
                nn("document"),
                vec![
                    RelationIr::direct(rn("parent"), vec![RelatedType::leaf(nn("folder"))]),
                    RelationIr::new(
                        rn("viewer"),
                        RewriteIr::TupleToUserset {
                            tupleset: rn("parent"),
                            computed: rn("viewer"),
                        },
                        vec![],
                    ),
                ],
            ))
            .build()
            .expect("valid schema"),
    )
}

#[tokio::test]
async fn engine_over_offline_store_resolves_tuple_to_userset() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");

    store
        .write(&[
            RelationTuple::new(
                obj("document", "readme"),
                rn("parent"),
                SubjectRef::leaf(nn("folder"), CanonicalId::new("root").unwrap()),
            ),
            RelationTuple::new(
                obj("folder", "root"),
                rn("viewer"),
                SubjectRef::leaf(nn("user"), CanonicalId::new("dave").unwrap()),
            ),
        ])
        .await
        .expect("write");

    let engine = Engine::new(store, schema());

    assert!(
        engine
            .check(
                obj("document", "readme"),
                rn("viewer"),
                SubjectRef::leaf(nn("user"), CanonicalId::new("dave").unwrap()),
                CaveatContext::new(),
                Consistency::default(),
            )
            .await
            .expect("check")
    );
    assert!(
        !engine
            .check(
                obj("document", "readme"),
                rn("viewer"),
                SubjectRef::leaf(nn("user"), CanonicalId::new("erin").unwrap()),
                CaveatContext::new(),
                Consistency::default(),
            )
            .await
            .expect("check")
    );
}
