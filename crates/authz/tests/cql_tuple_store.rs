//! Integration tests for the YCQL relation-tuple store; they need Docker.
#![cfg(feature = "cql")]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use fundamental_authz::backend::CqlTupleStore;
use fundamental_authz::schema::{CaveatName, CaveatValue, NamespaceName, RelationName};
use fundamental_authz::tuple::{
    CanonicalId, Caveat, CaveatContext, ClockScope, DirectEdge, ObjectRef, RelationTuple,
    SubjectRef, TupleStore,
};
use fundamental_test_containers::cql::{CqlKeyspace, CqlNode};

/// A fresh store on its own keyspace; bind the returned guard for the whole test.
async fn fresh_store() -> (CqlTupleStore, CqlKeyspace) {
    let node = CqlNode::shared().await.expect("a YCQL node");
    let keyspace = CqlKeyspace::mint(node).await.expect("a YCQL test keyspace");
    let store = CqlTupleStore::new(keyspace.pool().clone());
    store.ensure_schema().await.expect("ensure schema");
    (store, keyspace)
}

fn ns(name: &str) -> NamespaceName {
    NamespaceName::new(name).expect("valid namespace")
}
fn id(value: &str) -> CanonicalId {
    CanonicalId::new(value).expect("valid id")
}
fn rel(name: &str) -> RelationName {
    RelationName::new(name).expect("valid relation")
}
fn object(namespace: &str, ident: &str) -> ObjectRef {
    ObjectRef::new(ns(namespace), id(ident))
}

#[tokio::test]
async fn write_then_point_lookup_finds_exact_edge() {
    let (store, _keyspace) = fresh_store().await;
    let snap = ClockScope::current();
    let tuple = RelationTuple::new(
        object("document", "readme"),
        rel("viewer"),
        SubjectRef::leaf(ns("user"), id("alice")),
    );
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("write");

    let found = store
        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
        .await
        .expect("read_edge");
    assert_eq!(found, Some(DirectEdge::Unconditional));

    let absent = store
        .read_edge(
            &snap,
            &tuple.object,
            &tuple.relation,
            &SubjectRef::leaf(ns("user"), id("bob")),
        )
        .await
        .expect("read_edge");
    assert_eq!(absent, None);
}

#[tokio::test]
async fn caveated_edge_round_trips_its_caveat() {
    let (store, _keyspace) = fresh_store().await;
    let snap = ClockScope::current();
    let caveat = Caveat::new(
        CaveatName::new("not_expired").expect("valid"),
        CaveatContext::new().with("valid_until", CaveatValue::Timestamp(2000)),
    );
    let tuple = RelationTuple::new(
        object("document", "budget"),
        rel("editor"),
        SubjectRef::leaf(ns("user"), id("carol")),
    )
    .with_caveat(caveat.clone());
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("write");

    let found = store
        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
        .await
        .expect("read_edge");
    assert_eq!(found, Some(DirectEdge::Conditional(caveat)));
}

#[tokio::test]
async fn read_usersets_returns_only_userset_edges() {
    let (store, _keyspace) = fresh_store().await;
    let snap = ClockScope::current();
    let obj = object("document", "spec");
    let direct = RelationTuple::new(
        obj.clone(),
        rel("viewer"),
        SubjectRef::leaf(ns("user"), id("dave")),
    );
    let wildcard = RelationTuple::new(obj.clone(), rel("viewer"), SubjectRef::wildcard(ns("user")));
    let userset = RelationTuple::new(
        obj.clone(),
        rel("viewer"),
        SubjectRef::userset(ns("group"), id("eng"), rel("member")),
    );
    store
        .write(&[direct, wildcard, userset.clone()])
        .await
        .expect("write");

    let usersets = store
        .read_usersets(&snap, &obj, &rel("viewer"), 100)
        .await
        .expect("read_usersets");
    assert_eq!(
        usersets,
        vec![userset],
        "only the userset edge is returned (subject_kind='u' clustering prefix)"
    );
}

#[tokio::test]
async fn read_by_subject_reverse_lookup_with_filters() {
    let (store, _keyspace) = fresh_store().await;
    let snap = ClockScope::current();
    let alice = SubjectRef::leaf(ns("user"), id("alice"));
    let t1 = RelationTuple::new(object("document", "a"), rel("viewer"), alice.clone());
    let t2 = RelationTuple::new(object("document", "b"), rel("owner"), alice.clone());
    let t3 = RelationTuple::new(object("folder", "f"), rel("viewer"), alice.clone());
    store
        .write(&[t1.clone(), t2.clone(), t3.clone()])
        .await
        .expect("write");

    let mut all = store
        .read_by_subject(&snap, &alice, None, None, 100)
        .await
        .expect("reverse");
    all.sort_by_key(|t| t.to_string());
    assert_eq!(all.len(), 3);

    let docs = store
        .read_by_subject(
            &snap,
            &alice,
            Some(&ns("document")),
            Some(&rel("viewer")),
            100,
        )
        .await
        .expect("reverse filtered");
    assert_eq!(docs, vec![t1]);
}

#[tokio::test]
async fn write_is_an_upsert_and_delete_removes_from_both_tables() {
    let (store, _keyspace) = fresh_store().await;
    let snap = ClockScope::current();
    let base = RelationTuple::new(
        object("document", "x"),
        rel("editor"),
        SubjectRef::leaf(ns("user"), id("erin")),
    );
    store
        .write(std::slice::from_ref(&base))
        .await
        .expect("write 1");
    let caveated = base.clone().with_caveat(Caveat::new(
        CaveatName::new("business_hours").expect("valid"),
        CaveatContext::new(),
    ));
    store
        .write(std::slice::from_ref(&caveated))
        .await
        .expect("write 2");

    let found = store
        .read_edge(&snap, &base.object, &base.relation, &base.subject)
        .await
        .expect("read_edge");
    assert_eq!(
        found,
        Some(DirectEdge::Conditional(caveated.caveat.clone().unwrap())),
        "re-writing the same identity replaces the caveat (upsert)"
    );

    store
        .delete(std::slice::from_ref(&base))
        .await
        .expect("delete");

    let gone = store
        .read_edge(&snap, &base.object, &base.relation, &base.subject)
        .await
        .expect("read_edge");
    assert_eq!(gone, None);
    let reverse = store
        .read_by_subject(&snap, &base.subject, None, None, 100)
        .await
        .expect("reverse");
    assert!(reverse.is_empty(), "delete removed the reverse row too");
}
