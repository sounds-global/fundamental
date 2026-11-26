//! Integration tests for the SQL-wire relation-tuple store; they need Docker.
#![cfg(feature = "sql")]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_authz::AuthzError;
use fundamental_authz::authorizer::{Authorizer, NativeEngine};
use fundamental_authz::backend::SqlTupleStore;
use fundamental_authz::schema::{
    CaveatIr, CaveatName, CaveatOperand, CaveatValue, CompareOp, NamespaceIr, NamespaceName,
    RelatedType, RelationIr, RelationName, SchemaIr,
};
use fundamental_authz::tuple::{
    CanonicalId, Caveat, CaveatContext, DirectEdge, ObjectRef, RelationTuple, SubjectRef,
    TupleStore,
};
use fundamental_authz::zookie::Consistency;
use fundamental_db::IsolationLevel;
use fundamental_sql::{ClusterTime, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

/// A fresh store on its own database; bind the returned [`SqlSchema`] for the whole test.
async fn fresh_store() -> (SqlTupleStore, SqlSchema) {
    let (store, _pool, schema) = fresh_store_and_pool().await;
    (store, schema)
}

async fn fresh_store_and_pool() -> (SqlTupleStore, SqlPool, SqlSchema) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let store = SqlTupleStore::new(pool.clone());
    support::migrate_tuple_table(&pool).await;
    (store, pool, schema)
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
async fn strong_snapshot_reads_in_one_snapshot() {
    // A FullConsistency evaluation reads in one `REPEATABLE READ` snapshot.
    let (store, _database) = fresh_store().await;
    let tuple = RelationTuple::new(
        object("document", "spec"),
        rel("owner"),
        SubjectRef::leaf(ns("user"), id("frank")),
    );
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("write");

    store
        .evaluate(Consistency::FullConsistency, |strong| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                let found = store
                    .read_edge(&strong, &tuple.object, &tuple.relation, &tuple.subject)
                    .await
                    .expect("read_edge in the snapshot");
                assert_eq!(
                    found,
                    Some(DirectEdge::Unconditional),
                    "a strong snapshot read finds the committed edge"
                );
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

#[tokio::test]
async fn write_then_point_lookup_finds_exact_edge() {
    let (store, _database) = fresh_store().await;
    store
        .evaluate(Consistency::default(), |snap| {
            let store = &store;
            Box::pin(async move {
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
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

#[tokio::test]
async fn caveated_edge_round_trips_its_caveat() {
    let (store, _database) = fresh_store().await;
    store
        .evaluate(Consistency::default(), |snap| {
            let store = &store;
            Box::pin(async move {
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
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

#[tokio::test]
async fn read_usersets_returns_only_userset_edges() {
    let (store, _database) = fresh_store().await;
    store
        .evaluate(Consistency::default(), |snap| {
            let store = &store;
            Box::pin(async move {
                let obj = object("document", "spec");
                let direct = RelationTuple::new(
                    obj.clone(),
                    rel("viewer"),
                    SubjectRef::leaf(ns("user"), id("dave")),
                );
                let wildcard = RelationTuple::new(
                    obj.clone(),
                    rel("viewer"),
                    SubjectRef::wildcard(ns("user")),
                );
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
                assert_eq!(usersets, vec![userset], "only the userset edge is returned");
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

#[tokio::test]
async fn read_by_subject_reverse_lookup_with_filters() {
    let (store, _database) = fresh_store().await;
    store
        .evaluate(Consistency::default(), |snap| {
            let store = &store;
            Box::pin(async move {
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
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

#[tokio::test]
async fn write_is_an_upsert_and_delete_removes() {
    let (store, _database) = fresh_store().await;
    let base = RelationTuple::new(
        object("document", "x"),
        rel("editor"),
        SubjectRef::leaf(ns("user"), id("erin")),
    );
    let caveated = base.clone().with_caveat(Caveat::new(
        CaveatName::new("business_hours").expect("valid"),
        CaveatContext::new(),
    ));

    store
        .write(std::slice::from_ref(&base))
        .await
        .expect("write 1");
    store
        .write(std::slice::from_ref(&caveated))
        .await
        .expect("write 2");

    // Each observation gets its own scope: a scope is a snapshot, blind to later writes.
    let found = read_edge_in_a_fresh_scope(&store, &base).await;
    assert_eq!(
        found,
        Some(DirectEdge::Conditional(
            caveated.caveat.clone().expect("caveated")
        )),
        "re-writing the same identity replaces the caveat (upsert, not duplicate)"
    );

    store
        .delete(std::slice::from_ref(&base))
        .await
        .expect("delete");

    let gone = read_edge_in_a_fresh_scope(&store, &base).await;
    assert_eq!(gone, None, "the deleted edge is gone from a later snapshot");
}

/// Reads one edge inside its own evaluation scope, as a real check does.
async fn read_edge_in_a_fresh_scope(
    store: &SqlTupleStore,
    tuple: &RelationTuple,
) -> Option<DirectEdge> {
    store
        .evaluate(Consistency::default(), |snap| {
            let store = &store;
            Box::pin(async move {
                store
                    .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                    .await
            })
        })
        .await
        .expect("evaluate")
}

// ── Transactional writes: a resource and its authorization are one commit ───────

/// A resource row and its tuple are written in one transaction.
#[tokio::test]
async fn a_tuple_written_in_a_transaction_is_visible_once_it_commits() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let tuple = RelationTuple::new(
        object("credential", "committed"),
        rel("issuer"),
        SubjectRef::leaf(ns("issuer"), id("acme")),
    );

    pool.transaction(IsolationLevel::Serializable, async |tx| {
        SqlTupleStore::write_in(tx, std::slice::from_ref(&tuple)).await?;
        Ok::<_, AuthzError>(())
    })
    .await
    .expect("commits");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                assert_eq!(
                    store
                        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                        .await
                        .expect("read_edge"),
                    Some(DirectEdge::Unconditional),
                );
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

/// If the caller's work fails, the authorization it would have granted is gone with it.
#[tokio::test]
async fn a_tuple_written_in_a_transaction_disappears_when_it_rolls_back() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let tuple = RelationTuple::new(
        object("credential", "rolled-back"),
        rel("issuer"),
        SubjectRef::leaf(ns("issuer"), id("acme")),
    );

    let outcome: Result<(), AuthzError> = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::write_in(tx, std::slice::from_ref(&tuple)).await?;
            Err(AuthzError::SchemaSetup("the caller's work failed".into()))
        })
        .await;
    assert!(outcome.is_err(), "the closure's error propagates");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                assert_eq!(
                    store
                        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                        .await
                        .expect("read_edge"),
                    None,
                    "a rolled-back transaction must not leave the grant behind"
                );
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

/// `write_in` shares `write`'s tuple key, so the same identity twice is one row.
#[tokio::test]
async fn a_transactional_write_is_idempotent_on_the_tuple_identity() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let tuple = RelationTuple::new(
        object("credential", "twice"),
        rel("issuer"),
        SubjectRef::leaf(ns("issuer"), id("acme")),
    );

    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("pool write");
    pool.transaction(IsolationLevel::Serializable, async |tx| {
        SqlTupleStore::write_in(tx, std::slice::from_ref(&tuple)).await?;
        Ok::<_, AuthzError>(())
    })
    .await
    .expect("commits");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                let all = store
                    .read_all(&snap, &tuple.object, &tuple.relation, 100)
                    .await
                    .expect("read_all");
                assert_eq!(all.len(), 1, "one identity is one row: {all:?}");
                Ok(())
            })
        })
        .await
        .expect("evaluate");
}

/// Two reads in one evaluation cannot disagree across a write that lands between them.
///
/// The write is awaited between the two reads, so the test is deterministic.
#[tokio::test]
async fn two_reads_in_one_evaluation_cannot_straddle_a_write() {
    let (store, _database) = fresh_store().await;
    let tuple = RelationTuple::new(
        object("document", "straddle"),
        rel("editor"),
        SubjectRef::leaf(ns("user"), id("frank")),
    );
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("the edge exists before the evaluation opens");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                let before = store
                    .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                    .await
                    .expect("read one");
                assert!(before.is_some(), "the edge is there to begin with");

                store
                    .delete(std::slice::from_ref(tuple))
                    .await
                    .expect("the revoke commits");

                let after = store
                    .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                    .await
                    .expect("read two");

                assert_eq!(
                    before, after,
                    "one evaluation, one state: a revoke committed mid-check must not be \
                     visible to half of it"
                );
                Ok(())
            })
        })
        .await
        .expect("evaluate");

    // The revoke is real: a later evaluation sees it gone.
    let after_the_fact = read_edge_in_a_fresh_scope(&store, &tuple).await;
    assert_eq!(
        after_the_fact, None,
        "the revoke did land; a later evaluation observes it"
    );
}

// ── Transactional deletes: a resource and its authorization end together ────────

/// A row's removal and its tuples' revocation commit together.
#[tokio::test]
async fn a_tuple_deleted_in_a_transaction_is_gone_once_it_commits() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let tuple = RelationTuple::new(
        object("membership", "erased"),
        rel("owner"),
        SubjectRef::leaf(ns("member"), id("m-1")),
    );
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("granted");

    let removed = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::delete_in(tx, std::slice::from_ref(&tuple)).await
        })
        .await
        .expect("commits");
    assert_eq!(removed, 1, "the grant was not the row that was removed");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                assert_eq!(
                    store
                        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                        .await?,
                    None,
                    "the authority survived the transaction that revoked it"
                );
                Ok::<_, AuthzError>(())
            })
        })
        .await
        .expect("reads");
}

/// A rolled-back revocation leaves the authority intact.
#[tokio::test]
async fn a_revocation_rolled_back_leaves_the_authority_standing() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let tuple = RelationTuple::new(
        object("membership", "kept"),
        rel("owner"),
        SubjectRef::leaf(ns("member"), id("m-2")),
    );
    store
        .write(std::slice::from_ref(&tuple))
        .await
        .expect("granted");

    let rolled_back = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::delete_in(tx, std::slice::from_ref(&tuple)).await?;
            Err::<(), AuthzError>(AuthzError::UndefinedNamespace {
                namespace: "the row deletion failed".to_owned(),
            })
        })
        .await;
    assert!(rolled_back.is_err(), "the transaction did not roll back");

    store
        .evaluate(Consistency::FullConsistency, |snap| {
            let store = &store;
            let tuple = &tuple;
            Box::pin(async move {
                assert!(
                    store
                        .read_edge(&snap, &tuple.object, &tuple.relation, &tuple.subject)
                        .await?
                        .is_some(),
                    "a rolled-back revocation still took the authority away"
                );
                Ok::<_, AuthzError>(())
            })
        })
        .await
        .expect("reads");
}

/// Revoking what is already revoked removes nothing and is not an error.
#[tokio::test]
async fn revoking_an_absent_tuple_removes_nothing_and_does_not_fail() {
    let (_store, pool, _database) = fresh_store_and_pool().await;
    let never_granted = RelationTuple::new(
        object("membership", "absent"),
        rel("owner"),
        SubjectRef::leaf(ns("member"), id("m-3")),
    );

    let removed = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::delete_in(tx, std::slice::from_ref(&never_granted)).await
        })
        .await
        .expect("commits");
    assert_eq!(removed, 0);
}

fn windowed_schema() -> Arc<SchemaIr> {
    let now = || CaveatOperand::Param("now".into());
    let document = NamespaceIr::new(
        ns("document"),
        vec![RelationIr::direct(
            rel("owner"),
            vec![RelatedType::leaf(ns("user"))],
        )],
    )
    .with_caveats(BTreeMap::from([(
        CaveatName::new("on_duty").expect("valid"),
        CaveatIr::And(vec![
            CaveatIr::Compare {
                left: now(),
                op: CompareOp::Gte,
                right: CaveatOperand::Param("from".into()),
            },
            CaveatIr::Compare {
                left: now(),
                op: CompareOp::Lt,
                right: CaveatOperand::Param("until".into()),
            },
        ]),
    )]));
    Arc::new(
        SchemaIr::builder()
            .namespace_ir(NamespaceIr::new(ns("user"), vec![]))
            .namespace_ir(document)
            .build()
            .expect("valid schema"),
    )
}

/// A caveat's `now` is the cluster's clock as read by the evaluation, the zookie's instant.
#[tokio::test]
async fn now_is_the_databases_time() {
    let (store, pool, _database) = fresh_store_and_pool().await;
    let authz = NativeEngine::new(store, windowed_schema());
    let second = |wall_nanos: u64| i64::try_from(wall_nanos / 1_000_000_000).expect("fits");

    let opened = second(ClusterTime::read(&pool).await.expect("clock").nanos());
    let tuple = RelationTuple::new(
        object("document", "rota"),
        rel("owner"),
        SubjectRef::leaf(ns("user"), id("olga")),
    )
    .with_caveat(Caveat::new(
        CaveatName::new("on_duty").expect("valid"),
        CaveatContext::new()
            .with("from", CaveatValue::Timestamp(opened + 1))
            .with("until", CaveatValue::Timestamp(opened + 2)),
    ));
    Authorizer::write(&authz, &[tuple]).await.expect("write");

    let (mut admitted, mut refused) = (0, 0);
    loop {
        let response = Authorizer::check(
            &authz,
            object("document", "rota"),
            rel("owner"),
            SubjectRef::leaf(ns("user"), id("olga")),
            CaveatContext::new(),
            Consistency::FullConsistency,
        )
        .await
        .expect("check");
        let observed = second(response.evaluated_at.ts().wall);
        assert_eq!(
            response.allowed,
            observed == opened + 1,
            "the decision and the zookie must name one instant (zookie second {observed})"
        );
        if response.allowed {
            admitted += 1;
        } else {
            refused += 1;
        }
        if observed > opened + 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        admitted > 0 && refused > 0,
        "{admitted} admitted, {refused} refused"
    );
}
