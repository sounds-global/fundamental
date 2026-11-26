//! A locked check inside the act's own `READ COMMITTED` transaction; these need Docker.
#![cfg(feature = "sql")]
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fundamental_authz::AuthzError;
use fundamental_authz::authorizer::{
    LockedDecision, LockedQuestions, NativeEngine, TransactionalAuthorizer,
};
use fundamental_authz::backend::SqlTupleStore;
use fundamental_authz::schema::{
    CaveatIr, CaveatName, CaveatOperand, CaveatValue, CompareOp, Namespace, NamespaceExt,
    NamespaceIr, NamespaceName, Relation, RelationExt, RelationIr, RelationName, RewriteIr,
    SchemaIr,
};
use fundamental_authz::tuple::{
    CanonicalId, Caveat, CaveatContext, ObjectRef, RelationTuple, SubjectRef,
};
use fundamental_core::Error as _;
use fundamental_db::{IsolationLevel, Transaction as _, Transactional as _};
use fundamental_macros::Namespace;
use fundamental_sql::{ClusterTime, SqlPool, SqlTransaction};
use fundamental_test_containers::sql::SqlSchema;
use futures::channel::oneshot;
use tokio::time::{sleep, timeout};

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Group {
    #[relation(direct(User, Group::Member))]
    #[locked]
    Member,
}

#[derive(Namespace)]
enum Squad {
    #[relation(direct(User))]
    Member,
    #[relation(direct(User))]
    Banned,
    #[relation(exclusion(Member, Banned))]
    Active,
}

#[derive(Namespace)]
enum Team {
    #[relation(direct(User))]
    #[locked]
    Member,
    #[relation(direct(User))]
    #[locked]
    Beslisser,
    #[relation(intersection(Member, Beslisser))]
    #[locked]
    Decider,
    #[relation(direct(User, Group::Member))]
    #[locked]
    Viewer,
    #[relation(direct(User))]
    Guest,
}

#[derive(Namespace)]
enum Shift {
    #[relation(direct(User))]
    #[locked]
    Operator,
}

fn schema() -> Arc<SchemaIr> {
    let now = || CaveatOperand::Param("now".into());
    let on_duty = CaveatIr::Compare {
        left: now(),
        op: CompareOp::Lt,
        right: CaveatOperand::Param("until".into()),
    };
    Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace::<Group>()
            .namespace::<Squad>()
            .namespace::<Team>()
            .namespace_ir(Shift::ir().with_caveats(BTreeMap::from([(
                CaveatName::new("on_duty").expect("a valid caveat name"),
                on_duty,
            )])))
            .build()
            .expect("a valid schema"),
    )
}

/// A fresh store on its own schema; bind the returned [`SqlSchema`] for the whole test.
async fn fresh() -> (Arc<NativeEngine<SqlTupleStore>>, SqlPool, SqlSchema) {
    let database = SqlSchema::mint().await.expect("a test schema");
    let pool = database.pool().clone();
    let store = SqlTupleStore::new(pool.clone());
    support::migrate_tuple_table(&pool).await;
    (Arc::new(NativeEngine::new(store, schema())), pool, database)
}

fn rel(of: impl Relation) -> RelationName {
    RelationName::new(of.relation_name()).expect("a valid relation")
}

fn alice() -> SubjectRef {
    User::subject("alice").expect("a valid subject")
}

fn on_team(team: &str, relation: Team) -> RelationTuple {
    RelationTuple::new(
        Team::object(team).expect("a valid object"),
        rel(relation),
        alice(),
    )
}

fn asks(team: &str, relation: Team) -> LockedQuestions {
    LockedQuestions::can(alice(), relation, team).expect("a valid question")
}

async fn write(pool: &SqlPool, tuples: &[RelationTuple]) {
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        SqlTupleStore::write_in(tx, tuples).await
    })
    .await
    .expect("the tuples are written");
}

async fn revoke(pool: &SqlPool, tuples: &[RelationTuple]) {
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        SqlTupleStore::delete_in(tx, tuples).await
    })
    .await
    .expect("the tuples are revoked");
}

async fn begin(pool: &SqlPool) -> SqlTransaction {
    pool.begin(IsolationLevel::ReadCommitted)
        .await
        .expect("a READ COMMITTED transaction")
}

async fn act(
    authz: &NativeEngine<SqlTupleStore>,
    pool: &SqlPool,
    questions: LockedQuestions,
) -> Result<LockedDecision, AuthzError> {
    pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        authz.check_in_locked(tx, questions.clone()).await
    })
    .await
}

/// Whether another transaction can delete `tuple` within `wait`, i.e. nobody holds its lock.
async fn deletable_within(pool: &SqlPool, tuple: &RelationTuple, wait: Duration) -> bool {
    let mut other = begin(pool).await;
    let deleted = timeout(
        wait,
        SqlTupleStore::delete_in(&mut other, std::slice::from_ref(tuple)),
    )
    .await;
    match deleted {
        Ok(removed) => {
            removed.expect("the delete runs");
            other.rollback().await.expect("rollback");
            true
        }
        Err(_elapsed) => false,
    }
}

async fn database_seconds(pool: &SqlPool) -> i64 {
    let mut tx = begin(pool).await;
    let clock = ClusterTime::read_in(&mut tx)
        .await
        .expect("the cluster clock");
    tx.rollback().await.expect("rollback");
    i64::try_from(clock.micros() / 1_000_000).expect("seconds fit")
}

/// C1. A revocation committed before the act's check is not seen: the act is denied.
#[tokio::test]
async fn a_revocation_committed_before_the_check_denies() {
    let (authz, pool, _database) = fresh().await;
    write(&pool, &[on_team("a", Team::Member)]).await;
    revoke(&pool, &[on_team("a", Team::Member)]).await;

    let decision = act(&authz, &pool, asks("a", Team::Member))
        .await
        .expect("the act decides");
    assert!(!decision.all());
}

/// C2. A revocation issued while the act holds its locks waits until the act commits.
#[tokio::test]
async fn a_revocation_waits_for_the_act_and_a_later_act_is_denied() {
    let (authz, pool, _database) = fresh().await;
    let member = on_team("a", Team::Member);
    write(&pool, std::slice::from_ref(&member)).await;

    let (holding, held) = oneshot::channel::<()>();
    let the_act = async {
        let mut tx = begin(&pool).await;
        let decision = authz
            .check_in_locked(&mut tx, asks("a", Team::Member))
            .await
            .expect("the act decides");
        assert!(decision.all(), "the act is allowed");
        holding.send(()).expect("the revoker listens");
        sleep(Duration::from_millis(800)).await;
        // Taken before the commit: the lock is released before the client hears the acknowledgement.
        let committing = Instant::now();
        tx.commit().await.expect("the act commits");
        committing
    };
    let the_revocation = async {
        held.await.expect("the act holds its locks");
        let mut tx = begin(&pool).await;
        let removed = SqlTupleStore::delete_in(&mut tx, std::slice::from_ref(&member))
            .await
            .expect("the revocation runs");
        let revoked_at = Instant::now();
        tx.commit().await.expect("the revocation commits");
        (removed, revoked_at)
    };
    let (act_committed, (removed, revoked_at)) = tokio::join!(the_act, the_revocation);

    assert_eq!(removed, 1, "the revocation removed the tuple");
    assert!(
        revoked_at >= act_committed,
        "the revocation's delete returned before the act committed: it did not wait"
    );
    let later = act(&authz, &pool, asks("a", Team::Member))
        .await
        .expect("a later act decides");
    assert!(!later.all(), "a later act is denied");
}

/// C3. An uncommitted grant is not seen and takes nothing.
#[tokio::test]
async fn a_concurrent_grant_never_allows_what_was_not_committed() {
    let (authz, pool, _database) = fresh().await;
    let mut granter = begin(&pool).await;
    SqlTupleStore::write_in(&mut granter, &[on_team("a", Team::Member)])
        .await
        .expect("the grant is written, uncommitted");

    let before = timeout(
        Duration::from_secs(5),
        act(&authz, &pool, asks("a", Team::Member)),
    )
    .await
    .expect("a denial waits for nothing")
    .expect("the act decides");
    assert!(!before.all(), "an uncommitted grant does not allow");

    granter.commit().await.expect("the grant commits");
    let after = act(&authz, &pool, asks("a", Team::Member))
        .await
        .expect("the act decides");
    assert!(after.all(), "the committed grant allows");
}

/// C3. A witness revoked between the walk and its lock answers `AUTHZ_024`.
#[tokio::test]
async fn a_witness_revoked_in_flight_reruns_and_denies() {
    let (authz, pool, _database) = fresh().await;
    let member = on_team("a", Team::Member);
    write(&pool, std::slice::from_ref(&member)).await;

    let mut revoker = begin(&pool).await;
    SqlTupleStore::delete_in(&mut revoker, std::slice::from_ref(&member))
        .await
        .expect("the revocation holds the row, uncommitted");

    let attempts = AtomicUsize::new(0);
    let moved = AtomicUsize::new(0);
    let the_act = pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        attempts.fetch_add(1, Ordering::SeqCst);
        let answer = authz.check_in_locked(tx, asks("a", Team::Member)).await;
        if let Err(AuthzError::LockedWitnessMoved { .. }) = &answer {
            moved.fetch_add(1, Ordering::SeqCst);
        }
        answer
    });
    let the_revocation = async {
        sleep(Duration::from_millis(500)).await;
        revoker.commit().await.expect("the revocation commits");
    };
    let (decision, ()) = tokio::join!(the_act, the_revocation);

    let decision = decision.expect("the re-run decides");
    assert!(!decision.all(), "the re-run is denied");
    assert_eq!(
        moved.load(Ordering::SeqCst),
        1,
        "the first attempt's witness moved"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "the pool ran the act again"
    );
}

/// The error a moved witness answers is the retryable one the pool re-runs on.
#[test]
fn a_moved_witness_is_retryable() {
    let moved = AuthzError::LockedWitnessMoved {
        moved: 1,
        witness: 2,
    };
    assert_eq!(moved.error_code(), "AUTHZ_024");
    assert_eq!(moved.http_status_code(), 503);
    assert!(moved.is_retryable());
}

/// C4. A stored tuple of an undeclared shape cannot lead a locked walk into an exclusion.
#[tokio::test]
async fn a_locked_walk_meeting_an_undeclared_exclusion_errors() {
    let (authz, pool, _database) = fresh().await;
    let undeclared = RelationTuple::new(
        Team::object("a").expect("a valid object"),
        rel(Team::Viewer),
        Squad::Active.userset("s").expect("a valid userset"),
    );
    write(&pool, &[undeclared]).await;

    let refused = act(&authz, &pool, asks("a", Team::Viewer))
        .await
        .expect_err("the walk reached squad#active");
    assert_eq!(refused.error_code(), "AUTHZ_025");
    assert!(
        matches!(&refused, AuthzError::ExclusionInLockedWalk { namespace, relation }
            if namespace == "squad" && relation == "active"),
        "{refused:?}"
    );
}

/// C5. Any isolation other than READ COMMITTED is refused with `AUTHZ_023`.
#[tokio::test]
async fn repeatable_read_and_serializable_are_refused() {
    let (authz, pool, _database) = fresh().await;
    for level in [IsolationLevel::RepeatableRead, IsolationLevel::Serializable] {
        let refused = pool
            .transaction(level, async |tx| {
                authz.check_in_locked(tx, asks("a", Team::Member)).await
            })
            .await
            .expect_err("refused");
        assert_eq!(refused.error_code(), "AUTHZ_023", "{level:?}");
        assert!(
            matches!(&refused, AuthzError::LockedCheckIsolation { found } if found == level.as_sql()),
            "{refused:?}"
        );
    }
}

/// C5. A relation the schema did not declare `#[locked]` is refused with `AUTHZ_022`.
#[tokio::test]
async fn an_undeclared_relation_is_refused() {
    let (authz, pool, _database) = fresh().await;
    let refused = act(&authz, &pool, asks("a", Team::Guest))
        .await
        .expect_err("refused");
    assert_eq!(refused.error_code(), "AUTHZ_022");
}

/// C6. Two acts reading the same tuples in opposite orders never deadlock.
#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn mirrored_locked_checks_never_deadlock() {
    const ITERATIONS: usize = 100;
    let (authz, pool, _database) = fresh().await;
    write(
        &pool,
        &[on_team("a", Team::Member), on_team("b", Team::Member)],
    )
    .await;
    let forward = asks("a", Team::Member)
        .and_can(alice(), Team::Member, "b")
        .expect("a valid question");
    let backward = asks("b", Team::Member)
        .and_can(alice(), Team::Member, "a")
        .expect("a valid question");

    let attempts = AtomicUsize::new(0);
    let one_act = |questions: LockedQuestions| {
        let (authz, pool, attempts) = (&authz, &pool, &attempts);
        async move {
            pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
                attempts.fetch_add(1, Ordering::SeqCst);
                let decision = authz.check_in_locked(tx, questions.clone()).await?;
                sleep(Duration::from_millis(5)).await;
                Ok::<_, AuthzError>(decision)
            })
            .await
        }
    };
    for _ in 0..ITERATIONS {
        let (one, two) = tokio::join!(one_act(forward.clone()), one_act(backward.clone()));
        assert!(one.expect("the forward act commits").all());
        assert!(two.expect("the backward act commits").all());
    }
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2 * ITERATIONS,
        "an act was re-run: a lock cycle or a moved witness"
    );
}

/// C7. A denied check takes no lock: what it read can be revoked while the act is open.
#[tokio::test]
async fn a_denied_check_takes_no_lock() {
    let (authz, pool, _database) = fresh().await;
    let member = on_team("a", Team::Member);
    write(&pool, std::slice::from_ref(&member)).await;

    let mut tx = begin(&pool).await;
    let decision = authz
        .check_in_locked(&mut tx, asks("a", Team::Decider))
        .await
        .expect("the act decides");
    assert!(
        !decision.all(),
        "a member who is no beslisser is no decider"
    );
    assert!(
        deletable_within(&pool, &member, Duration::from_secs(3)).await,
        "the member tuple the denied walk read was locked"
    );
    tx.rollback().await.expect("rollback");
}

/// C7. Only the tuples the allow rests on are locked.
#[tokio::test]
async fn a_tuple_outside_the_witness_is_not_locked() {
    let (authz, pool, _database) = fresh().await;
    let through = |group: &str| {
        RelationTuple::new(
            Team::object("a").expect("a valid object"),
            rel(Team::Viewer),
            Group::Member.userset(group).expect("a valid userset"),
        )
    };
    let in_g2 = RelationTuple::new(
        Group::object("g2").expect("a valid object"),
        rel(Group::Member),
        alice(),
    );
    write(&pool, &[through("g1"), through("g2"), in_g2.clone()]).await;

    let mut tx = begin(&pool).await;
    let decision = authz
        .check_in_locked(&mut tx, asks("a", Team::Viewer))
        .await
        .expect("the act decides");
    assert!(decision.all(), "alice views team a through g2");

    assert!(
        deletable_within(&pool, &through("g1"), Duration::from_secs(3)).await,
        "team a's grant to g1 was read but is no part of the allow"
    );
    assert!(
        !deletable_within(&pool, &through("g2"), Duration::from_millis(1500)).await,
        "team a's grant to g2 is part of the allow"
    );
    assert!(
        !deletable_within(&pool, &in_g2, Duration::from_millis(1500)).await,
        "alice's membership of g2 is part of the allow"
    );
    tx.rollback().await.expect("rollback");
}

/// C8. A window that closes while the act waits for the lock answers `AUTHZ_024`.
#[tokio::test]
async fn a_window_closing_during_the_lock_wait_denies() {
    let (authz, pool, _database) = fresh().await;
    let until = database_seconds(&pool).await + 2;
    let operator = RelationTuple::new(
        Shift::object("night").expect("a valid object"),
        rel(Shift::Operator),
        alice(),
    )
    .with_caveat(Caveat::new(
        CaveatName::new("on_duty").expect("a valid caveat name"),
        CaveatContext::new().with("until", CaveatValue::Timestamp(until)),
    ));
    write(&pool, std::slice::from_ref(&operator)).await;

    let mut holder = begin(&pool).await;
    SqlTupleStore::write_in(&mut holder, std::slice::from_ref(&operator))
        .await
        .expect("the holder locks the tuple");

    let attempts = AtomicUsize::new(0);
    let moved = AtomicUsize::new(0);
    let the_act = pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
        attempts.fetch_add(1, Ordering::SeqCst);
        let answer = authz
            .check_in_locked(
                tx,
                LockedQuestions::can(alice(), Shift::Operator, "night").expect("a valid question"),
            )
            .await;
        if let Err(AuthzError::LockedWitnessMoved { .. }) = &answer {
            moved.fetch_add(1, Ordering::SeqCst);
        }
        answer
    });
    let the_holder = async {
        sleep(Duration::from_millis(3500)).await;
        holder.commit().await.expect("the holder commits");
    };
    let (decision, ()) = tokio::join!(the_act, the_holder);

    let decision = decision.expect("the re-run decides");
    assert!(
        !decision.all(),
        "the window closed before the lock was granted"
    );
    assert_eq!(moved.load(Ordering::SeqCst), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

/// C9. A witness over the bound is refused with `AUTHZ_026` rather than locked.
#[tokio::test]
async fn a_witness_over_the_bound_is_refused() {
    let database = SqlSchema::mint().await.expect("a test schema");
    let pool = database.pool().clone();
    let store = SqlTupleStore::new(pool.clone());
    support::migrate_tuple_table(&pool).await;

    let ns = |n: &str| NamespaceName::new(n).expect("a valid namespace");
    let part = |i: usize| RelationName::new(format!("part_{i}")).expect("a valid relation");
    let parts = LockedQuestions::WITNESS_LIMIT + 1;
    let mut relations: Vec<RelationIr> = (0..parts)
        .map(|i| {
            RelationIr::direct(
                part(i),
                vec![fundamental_authz::schema::RelatedType::leaf(ns("user"))],
            )
        })
        .collect();
    relations.push(
        RelationIr::new(
            RelationName::new("whole").expect("a valid relation"),
            RewriteIr::Intersection(
                (0..parts)
                    .map(|i| RewriteIr::ComputedUserset { relation: part(i) })
                    .collect(),
            ),
            vec![],
        )
        .with_lock(),
    );
    let schema = SchemaIr::builder()
        .namespace::<User>()
        .namespace_ir(NamespaceIr::new(ns("ledger"), relations))
        .build()
        .expect("a valid schema");
    let authz = NativeEngine::new(store, Arc::new(schema));

    let ledger = ObjectRef::new(ns("ledger"), CanonicalId::new("l").expect("a valid id"));
    let grants: Vec<RelationTuple> = (0..parts)
        .map(|i| RelationTuple::new(ledger.clone(), part(i), alice()))
        .collect();
    write(&pool, &grants).await;

    let refused = act(
        &authz,
        &pool,
        LockedQuestions::new(
            ledger,
            RelationName::new("whole").expect("a valid relation"),
            alice(),
        ),
    )
    .await
    .expect_err("refused");
    assert_eq!(refused.error_code(), "AUTHZ_026");
    assert!(
        matches!(refused, AuthzError::LockBudgetExceeded { tuples, limit }
            if tuples == parts && limit == LockedQuestions::WITNESS_LIMIT)
    );
}

/// C10. A locked check while a savepoint is open is refused with `AUTHZ_027`.
#[tokio::test]
async fn a_locked_check_inside_a_savepoint_is_refused() {
    let (authz, pool, _database) = fresh().await;
    write(&pool, &[on_team("a", Team::Member)]).await;

    let mut tx = begin(&pool).await;
    tx.execute_savepoint("before").await.expect("savepoint");
    let refused = authz
        .check_in_locked(&mut tx, asks("a", Team::Member))
        .await
        .expect_err("refused inside the savepoint");
    assert_eq!(refused.error_code(), "AUTHZ_027");
    assert!(matches!(
        refused,
        AuthzError::LockedCheckInSavepoint { open: 1 }
    ));

    tx.execute_rollback_to_savepoint("before")
        .await
        .expect("rollback to the savepoint");
    let still = authz
        .check_in_locked(&mut tx, asks("a", Team::Member))
        .await
        .expect_err("a rollback leaves the savepoint open");
    assert_eq!(still.error_code(), "AUTHZ_027");

    tx.execute_release_savepoint("before")
        .await
        .expect("release");
    let decision = authz
        .check_in_locked(&mut tx, asks("a", Team::Member))
        .await
        .expect("admitted once released");
    assert!(decision.all());
    tx.rollback().await.expect("rollback");
}

/// The questions of one call share one decision, in the order asked.
#[tokio::test]
async fn one_call_answers_each_question_in_order() {
    let (authz, pool, _database) = fresh().await;
    write(&pool, &[on_team("a", Team::Member)]).await;
    let decision = act(
        &authz,
        &pool,
        asks("a", Team::Member)
            .and_can(alice(), Team::Beslisser, "a")
            .expect("a valid question"),
    )
    .await
    .expect("the act decides");
    assert_eq!(decision.iter().collect::<Vec<_>>(), vec![true, false]);
    assert!(!decision.all());
}

/// Errors other than the moved witness keep their own codes and statuses.
#[test]
fn the_locked_check_errors_carry_their_codes() {
    let cases: [(AuthzError, &str); 4] = [
        (
            AuthzError::RelationNotLocked {
                namespace: "team".into(),
                relation: "guest".into(),
            },
            "AUTHZ_022",
        ),
        (
            AuthzError::LockedCheckIsolation {
                found: "SERIALIZABLE".into(),
            },
            "AUTHZ_023",
        ),
        (
            AuthzError::LockBudgetExceeded {
                tuples: 65,
                limit: 64,
            },
            "AUTHZ_026",
        ),
        (AuthzError::LockedCheckInSavepoint { open: 1 }, "AUTHZ_027"),
    ];
    for (error, code) in cases {
        assert_eq!(error.error_code(), code);
        assert_eq!(error.http_status_code(), 500);
        assert!(!error.is_retryable(), "{code}");
    }
}

/// A public grant (`user:*`) is a witness like any other.
#[tokio::test]
async fn a_wildcard_grant_is_locked_and_allows_first_time() {
    let (authz, pool, _database) = fresh().await;
    let public = RelationTuple::new(
        Team::object("a").expect("a valid object"),
        rel(Team::Member),
        User::wildcard(),
    );
    write(&pool, std::slice::from_ref(&public)).await;

    let mut tx = begin(&pool).await;
    let decision = authz
        .check_in_locked(&mut tx, asks("a", Team::Member))
        .await
        .expect("allowed without a moved witness");
    assert!(decision.all());
    assert!(
        !deletable_within(&pool, &public, Duration::from_millis(1500)).await,
        "the public grant the allow rests on is locked"
    );
    tx.rollback().await.expect("rollback");
}

/// A caveated grant whose window is open is a witness like any other.
#[tokio::test]
async fn a_caveated_grant_is_locked_and_allows_first_time() {
    let (authz, pool, _database) = fresh().await;
    let until = database_seconds(&pool).await + 600;
    let operator = RelationTuple::new(
        Shift::object("day").expect("a valid object"),
        rel(Shift::Operator),
        alice(),
    )
    .with_caveat(Caveat::new(
        CaveatName::new("on_duty").expect("a valid caveat name"),
        CaveatContext::new().with("until", CaveatValue::Timestamp(until)),
    ));
    write(&pool, std::slice::from_ref(&operator)).await;

    let attempts = AtomicUsize::new(0);
    let decision = pool
        .transaction(IsolationLevel::ReadCommitted, async |tx| {
            attempts.fetch_add(1, Ordering::SeqCst);
            authz
                .check_in_locked(
                    tx,
                    LockedQuestions::can(alice(), Shift::Operator, "day")
                        .expect("a valid question"),
                )
                .await
        })
        .await
        .expect("the act decides");
    assert!(decision.all());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "the witness did not move"
    );
}

/// C6. The store's own multi-tuple revocation locks in the same key order, so never deadlocks.
#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn mirrored_locked_check_and_bulk_revocation_never_deadlock() {
    use fundamental_authz::tuple::TupleStore as _;

    const ITERATIONS: usize = 40;
    let (authz, pool, _database) = fresh().await;
    let store = SqlTupleStore::new(pool.clone());
    let (member, beslisser) = (on_team("a", Team::Member), on_team("a", Team::Beslisser));
    let both = asks("a", Team::Member)
        .and_can(alice(), Team::Beslisser, "a")
        .expect("a valid question");

    let attempts = AtomicUsize::new(0);
    let moved = AtomicUsize::new(0);
    for _ in 0..ITERATIONS {
        write(&pool, &[member.clone(), beslisser.clone()]).await;
        let the_act = pool.transaction(IsolationLevel::ReadCommitted, async |tx| {
            attempts.fetch_add(1, Ordering::SeqCst);
            let decision = match authz.check_in_locked(tx, both.clone()).await {
                Err(error @ AuthzError::LockedWitnessMoved { .. }) => {
                    moved.fetch_add(1, Ordering::SeqCst);
                    return Err(error);
                }
                answer => answer?,
            };
            sleep(Duration::from_millis(10)).await;
            Ok::<_, AuthzError>(decision)
        });
        let the_revocation = async {
            sleep(Duration::from_millis(3)).await;
            store.delete(&[beslisser.clone(), member.clone()]).await
        };
        let (decision, revoked) = tokio::join!(the_act, the_revocation);
        let _either_answer = decision.expect("the act commits");
        revoked.expect("the revocation commits");
    }
    // A revocation committed before the act's lock re-runs the act; any more re-runs are a cycle.
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        ITERATIONS + moved.load(Ordering::SeqCst),
        "an act was re-run for a reason other than a moved witness: a lock cycle"
    );
}
