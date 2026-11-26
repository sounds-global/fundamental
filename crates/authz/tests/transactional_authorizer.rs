//! A decision taken inside the caller's transaction (`TransactionalAuthorizer`); needs Docker.
#![cfg(feature = "sql")]
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_authz::authorizer::{DynAuthorizer, NativeEngine, TransactionalAuthorizer};
use fundamental_authz::backend::SqlTupleStore;
use fundamental_authz::engine::{Engine, SetIndex};
use fundamental_authz::schema::{
    CaveatIr, CaveatName, CaveatOperand, CaveatValue, CompareOp, Namespace, NamespaceExt,
    NamespaceName, Relation, RelationName, SchemaIr,
};
use fundamental_authz::tuple::{Caveat, CaveatContext, ObjectRef, RelationTuple, SubjectRef};
use fundamental_authz::zookie::{Consistency, HybridTs};
use fundamental_authz::{AuthzError, authz_schema};
use fundamental_core::Error as _;
use fundamental_db::{DatabaseError, IsolationLevel};
use fundamental_macros::Namespace;
use fundamental_sql::{ClusterTime, SqlPool};
use fundamental_test_containers::sql::SqlSchema;

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
    Arc::new(authz_schema![User, Document].expect("a valid schema"))
}

/// A fresh store on its own schema and the pool; bind the returned [`SqlSchema`].
async fn fresh() -> (SqlTupleStore, SqlPool, SqlSchema) {
    let schema = SqlSchema::mint().await.expect("a test schema");
    let pool = schema.pool().clone();
    let store = SqlTupleStore::new(pool.clone());
    support::migrate_tuple_table(&pool).await;
    (store, pool, schema)
}

fn readme() -> ObjectRef {
    Document::object("readme").expect("a valid object")
}

fn relation(of: Document) -> RelationName {
    RelationName::new(of.relation_name()).expect("a valid relation")
}

fn alice() -> SubjectRef {
    User::subject("alice").expect("a valid subject")
}

fn ownership() -> RelationTuple {
    RelationTuple::new(readme(), relation(Document::Owner), alice())
}

/// A decision inside the transaction sees the writes it has already made.
#[tokio::test]
async fn an_expand_inside_a_transaction_sees_its_own_writes() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::new(store, schema());

    let owners = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::write_in(tx, std::slice::from_ref(&ownership())).await?;
            let tree = authz
                .expand_in(tx, readme(), relation(Document::Owner))
                .await?;
            Ok::<_, AuthzError>(tree.all_subjects())
        })
        .await
        .expect("the transaction commits");

    assert_eq!(
        owners,
        vec![alice()],
        "the owner written a statement earlier is invisible to the decision beside it"
    );
}

/// A revocation inside the transaction denies inside it, while a separate snapshot still allows.
#[tokio::test]
async fn a_check_inside_a_transaction_sees_its_own_revocation() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::new(store, schema());
    DynAuthorizer::write(&authz, std::slice::from_ref(&ownership()))
        .await
        .expect("the owner exists before the transaction opens");

    let (inside, outside) = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            SqlTupleStore::delete_in(tx, std::slice::from_ref(&ownership())).await?;
            let inside = authz
                .check_in(
                    tx,
                    readme(),
                    relation(Document::Viewer),
                    alice(),
                    CaveatContext::new(),
                )
                .await?;
            let outside = DynAuthorizer::check(
                &authz,
                readme(),
                relation(Document::Viewer),
                alice(),
                CaveatContext::new(),
                Consistency::FullConsistency,
            )
            .await?
            .allowed;
            Ok::<_, AuthzError>((inside, outside))
        })
        .await
        .expect("the transaction commits");

    assert!(
        !inside,
        "the revocation is part of the decision's own state"
    );
    assert!(
        outside,
        "a snapshot of its own cannot see an uncommitted revocation — the gap this closes"
    );
}

/// Below `SERIALIZABLE` the decision is refused, with its code, and not retried.
#[tokio::test]
async fn a_decision_refuses_a_transaction_weaker_than_serializable() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::new(store, schema());

    let refused = pool
        .transaction(IsolationLevel::RepeatableRead, async |tx| {
            authz
                .check_in(
                    tx,
                    readme(),
                    relation(Document::Owner),
                    alice(),
                    CaveatContext::new(),
                )
                .await
        })
        .await
        .expect_err("a REPEATABLE READ transaction cannot host a decision");

    assert!(
        matches!(refused, AuthzError::IsolationTooWeak { .. }),
        "refused as {refused:?}"
    );
    assert_eq!(refused.error_code(), "AUTHZ_017");
    assert!(
        !refused.is_retryable(),
        "a weaker isolation does not become stronger by waiting"
    );
}

/// An index that says yes to everything.
#[derive(Debug)]
struct AnswersYes;

impl SetIndex for AnswersYes {
    fn watermark(&self) -> HybridTs {
        HybridTs::ZERO
    }

    fn covers(&self, _namespace: &NamespaceName, _relation: &RelationName) -> bool {
        true
    }

    fn contains(
        &self,
        _object: &ObjectRef,
        _relation: &RelationName,
        _subject: &SubjectRef,
    ) -> Option<bool> {
        Some(true)
    }
}

/// An in-transaction decision never consults the set index; the pool check shows it would.
#[tokio::test]
async fn the_set_index_is_never_consulted_inside_a_transaction() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::from_engine(
        Engine::new(store, schema()).with_set_index(Arc::new(AnswersYes)),
    );

    let from_the_index = DynAuthorizer::check(
        &authz,
        readme(),
        relation(Document::Owner),
        alice(),
        CaveatContext::new(),
        Consistency::MinimizeLatency,
    )
    .await
    .expect("the pool check completes")
    .allowed;
    assert!(
        from_the_index,
        "the lying index answers a latency-optimised pool check"
    );

    let from_the_tuples = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            authz
                .check_in(
                    tx,
                    readme(),
                    relation(Document::Owner),
                    alice(),
                    CaveatContext::new(),
                )
                .await
        })
        .await
        .expect("the transaction commits");
    assert!(
        !from_the_tuples,
        "no tuple grants this, so a decision that allowed it read the index"
    );
}

fn windowed_schema() -> Arc<SchemaIr> {
    let now = || CaveatOperand::Param("now".into());
    let on_duty = CaveatIr::And(vec![
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
    ]);
    Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace_ir(Document::ir().with_caveats(BTreeMap::from([(
                CaveatName::new("on_duty").expect("a valid caveat name"),
                on_duty,
            )])))
            .build()
            .expect("a valid schema"),
    )
}

fn owner_between(id: &str, from: i64, until: i64) -> RelationTuple {
    RelationTuple::new(
        Document::object(id).expect("a valid object"),
        relation(Document::Owner),
        alice(),
    )
    .with_caveat(Caveat::new(
        CaveatName::new("on_duty").expect("a valid caveat name"),
        CaveatContext::new()
            .with("from", CaveatValue::Timestamp(from))
            .with("until", CaveatValue::Timestamp(until)),
    ))
}

/// An in-transaction decision judges a window at the cluster's clock, not the zero instant.
#[tokio::test]
async fn now_inside_a_transaction_is_the_databases_time() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::new(store, windowed_schema());

    let (current, ended, future, expanded) = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            let clock = ClusterTime::read_in(tx)
                .await
                .map_err(|e| DatabaseError::query(e.to_string()))?;
            let t = i64::try_from(clock.micros() / 1_000_000).expect("seconds fit");
            SqlTupleStore::write_in(
                tx,
                &[
                    owner_between("current", t, t + 600),
                    owner_between("ended", t - 600, t),
                    owner_between("future", t + 600, t + 1200),
                ],
            )
            .await?;
            let mut answers = Vec::new();
            for id in ["current", "ended", "future"] {
                answers.push(
                    authz
                        .check_in(
                            tx,
                            Document::object(id).expect("a valid object"),
                            relation(Document::Owner),
                            alice(),
                            CaveatContext::new(),
                        )
                        .await?,
                );
            }
            let expanded = authz
                .expand_in(
                    tx,
                    Document::object("ended").expect("a valid object"),
                    relation(Document::Owner),
                )
                .await?
                .all_subjects();
            Ok::<_, AuthzError>((answers[0], answers[1], answers[2], expanded))
        })
        .await
        .expect("the transaction commits");

    assert!(current, "the window open at the database's time admits");
    assert!(
        !ended,
        "the window that closed at the database's time denies"
    );
    assert!(!future, "the window that has not opened denies");
    assert!(expanded.is_empty(), "an ended window leaves the expansion");
}

/// The transactional path refuses a request binding exactly as the snapshot path does.
#[tokio::test]
async fn a_caller_cannot_supply_now_inside_a_transaction() {
    let (store, pool, _database) = fresh().await;
    let authz = NativeEngine::new(store, windowed_schema());
    let refused = pool
        .transaction(IsolationLevel::Serializable, async |tx| {
            authz
                .check_in(
                    tx,
                    readme(),
                    relation(Document::Owner),
                    alice(),
                    CaveatContext::new().with("now", CaveatValue::Timestamp(0)),
                )
                .await
        })
        .await
        .expect_err("refused");
    assert_eq!(refused.error_code(), "AUTHZ_019");
}
