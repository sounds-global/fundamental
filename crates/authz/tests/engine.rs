//! The check evaluation engine against an in-memory store.
#![allow(missing_docs)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fundamental_authz::AuthzError;
use fundamental_authz::engine::{Engine, EvalLimits, TransitiveClosureIndex};
use fundamental_authz::schema::{
    CaveatIr, CaveatName, CaveatOperand, CaveatValue, CompareOp, NamespaceIr, NamespaceName,
    RelatedType, RelationIr, RelationName, RewriteIr, SchemaIr,
};
use fundamental_authz::tuple::{
    CanonicalId, Caveat, CaveatContext, ClockScope, DirectEdge, ObjectRef, RelationTuple,
    ScopedEval, SubjectRef, TupleStore, WallTime,
};
use fundamental_authz::zookie::{BackendTag, Consistency, HybridClock, HybridTs, Zookie};
use fundamental_core::Error as _;

struct MemoryStore {
    tuples: Mutex<Vec<RelationTuple>>,
    now: Mutex<Option<WallTime>>,
}

impl MemoryStore {
    fn new(tuples: Vec<RelationTuple>) -> Self {
        Self {
            tuples: Mutex::new(tuples),
            now: Mutex::new(None),
        }
    }

    fn set_now(&self, now: WallTime) {
        *self.now.lock().unwrap() = Some(now);
    }
}

impl TupleStore for MemoryStore {
    fn backend(&self) -> BackendTag {
        BackendTag::Offline
    }

    type Scope = ClockScope;

    async fn evaluate<'a, F, T>(
        &'a self,
        _consistency: Consistency,
        body: F,
    ) -> Result<T, AuthzError>
    where
        F: Fn(Arc<Self::Scope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a,
    {
        let now = self.now.lock().unwrap().unwrap_or_else(WallTime::now);
        body(Arc::new(ClockScope::new(HybridClock::now(), now))).await
    }

    async fn read_edge(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        let tuples = self.tuples.lock().unwrap();
        Ok(tuples
            .iter()
            .find(|t| &t.object == object && &t.relation == relation && &t.subject == subject)
            .map(|t| DirectEdge::from_caveat(t.caveat.clone())))
    }

    async fn read_usersets(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let tuples = self.tuples.lock().unwrap();
        Ok(tuples
            .iter()
            .filter(|t| &t.object == object && &t.relation == relation && t.subject.is_userset())
            .take(limit)
            .cloned()
            .collect())
    }

    async fn read_all(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let tuples = self.tuples.lock().unwrap();
        Ok(tuples
            .iter()
            .filter(|t| &t.object == object && &t.relation == relation)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn read_by_subject(
        &self,
        _scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        let tuples = self.tuples.lock().unwrap();
        Ok(tuples
            .iter()
            .filter(|t| &t.subject == subject)
            .filter(|t| object_namespace.is_none_or(|ns| &t.object.namespace == ns))
            .filter(|t| relation.is_none_or(|r| &t.relation == r))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn write(&self, incoming: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let mut tuples = self.tuples.lock().unwrap();
        for tuple in incoming {
            tuples.retain(|t| t.key() != tuple.key());
            tuples.push(tuple.clone());
        }
        Ok(HybridClock::now())
    }

    async fn delete(&self, outgoing: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        let mut tuples = self.tuples.lock().unwrap();
        for tuple in outgoing {
            tuples.retain(|t| t.key() != tuple.key());
        }
        Ok(HybridClock::now())
    }
}

fn nn(s: &str) -> NamespaceName {
    NamespaceName::new(s).expect("namespace")
}
fn rn(s: &str) -> RelationName {
    RelationName::new(s).expect("relation")
}
fn cn(s: &str) -> CaveatName {
    CaveatName::new(s).expect("caveat")
}
fn obj(ns: &str, id: &str) -> ObjectRef {
    ObjectRef::new(nn(ns), CanonicalId::new(id).expect("id"))
}
fn user(id: &str) -> SubjectRef {
    SubjectRef::leaf(nn("user"), CanonicalId::new(id).expect("id"))
}
fn computed(relation: &str) -> RewriteIr {
    RewriteIr::ComputedUserset {
        relation: rn(relation),
    }
}
fn tuple(object: ObjectRef, relation: &str, subject: SubjectRef) -> RelationTuple {
    RelationTuple::new(object, rn(relation), subject)
}

/// document / folder / group / user with a `not_expired` caveat on `document`.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn schema() -> Arc<SchemaIr> {
    let user = NamespaceIr::new(nn("user"), vec![]);

    let group = NamespaceIr::new(
        nn("group"),
        vec![RelationIr::direct(
            rn("member"),
            vec![
                RelatedType::leaf(nn("user")),
                RelatedType::userset(nn("group"), rn("member")),
            ],
        )],
    );

    let folder = NamespaceIr::new(
        nn("folder"),
        vec![
            RelationIr::direct(rn("owner"), vec![RelatedType::leaf(nn("user"))]),
            RelationIr::new(
                rn("viewer"),
                RewriteIr::Union(vec![computed("owner"), RewriteIr::This]),
                vec![RelatedType::leaf(nn("user"))],
            ),
        ],
    );

    let document = NamespaceIr::new(
        nn("document"),
        vec![
            RelationIr::direct(
                rn("owner"),
                vec![
                    RelatedType::leaf(nn("user")),
                    RelatedType::userset(nn("group"), rn("member")),
                ],
            ),
            RelationIr::new(
                rn("editor"),
                RewriteIr::Union(vec![computed("owner"), RewriteIr::This]),
                vec![RelatedType::leaf(nn("user"))],
            ),
            RelationIr::direct(rn("parent"), vec![RelatedType::leaf(nn("folder"))]),
            RelationIr::direct(rn("banned"), vec![RelatedType::leaf(nn("user"))]),
            RelationIr::new(
                rn("viewer"),
                RewriteIr::Exclusion {
                    include: Box::new(RewriteIr::Union(vec![
                        computed("editor"),
                        RewriteIr::TupleToUserset {
                            tupleset: rn("parent"),
                            computed: rn("viewer"),
                        },
                        RewriteIr::This,
                    ])),
                    exclude: Box::new(computed("banned")),
                },
                vec![RelatedType::leaf(nn("user"))],
            ),
        ],
    )
    .with_caveats(BTreeMap::from([
        (
            cn("not_expired"),
            CaveatIr::Compare {
                left: CaveatOperand::Param("now".into()),
                op: CompareOp::Lt,
                right: CaveatOperand::Param("valid_until".into()),
            },
        ),
        (cn("on_duty"), on_duty()),
    ]));

    Arc::new(
        SchemaIr::builder()
            .namespace_ir(user)
            .namespace_ir(group)
            .namespace_ir(folder)
            .namespace_ir(document)
            .build()
            .expect("valid schema"),
    )
}

/// `now >= from && now < until` — a window, the shape a shift or a temporary share takes.
fn on_duty() -> CaveatIr {
    CaveatIr::And(vec![
        CaveatIr::Compare {
            left: CaveatOperand::Param("now".into()),
            op: CompareOp::Gte,
            right: CaveatOperand::Param("from".into()),
        },
        CaveatIr::Compare {
            left: CaveatOperand::Param("now".into()),
            op: CompareOp::Lt,
            right: CaveatOperand::Param("until".into()),
        },
    ])
}

/// 2026-10-01T09:00:00Z.
const SHIFT_FROM: i64 = 1_790_845_200;
const SHIFT_UNTIL: i64 = SHIFT_FROM + 8 * 3600;

/// Erin is an editor of `document:rota` for the shift, and for nothing else.
fn on_shift(stored: CaveatContext) -> RelationTuple {
    tuple(obj("document", "rota"), "editor", user("erin"))
        .with_caveat(Caveat::new(cn("on_duty"), stored))
}

fn the_shift() -> CaveatContext {
    CaveatContext::new()
        .with("from", CaveatValue::Timestamp(SHIFT_FROM))
        .with("until", CaveatValue::Timestamp(SHIFT_UNTIL))
}

fn at(unix_seconds: i64) -> WallTime {
    WallTime::from_unix_seconds(u64::try_from(unix_seconds).expect("after the epoch"))
}

async fn erin_may_edit(
    e: &Engine<MemoryStore>,
    context: CaveatContext,
) -> Result<bool, AuthzError> {
    e.check(
        obj("document", "rota"),
        rn("editor"),
        user("erin"),
        context,
        Consistency::default(),
    )
    .await
}

fn engine(tuples: Vec<RelationTuple>) -> Engine<MemoryStore> {
    Engine::new(MemoryStore::new(tuples), schema())
}

async fn allowed(
    engine: &Engine<MemoryStore>,
    object: ObjectRef,
    relation: &str,
    subject: SubjectRef,
) -> bool {
    engine
        .check(
            object,
            rn(relation),
            subject,
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
        .expect("check")
}

#[tokio::test]
async fn direct_relation_grants_and_denies() {
    let e = engine(vec![tuple(obj("document", "readme"), "owner", user("bob"))]);
    assert!(allowed(&e, obj("document", "readme"), "owner", user("bob")).await);
    assert!(!allowed(&e, obj("document", "readme"), "owner", user("eve")).await);
}

#[tokio::test]
async fn computed_userset_folds_owner_into_editor() {
    let e = engine(vec![tuple(obj("document", "readme"), "owner", user("bob"))]);
    assert!(allowed(&e, obj("document", "readme"), "editor", user("bob")).await);
}

#[tokio::test]
async fn userset_membership_grants_indirectly() {
    let e = engine(vec![
        tuple(obj("group", "eng"), "member", user("alice")),
        RelationTuple::new(
            obj("document", "readme"),
            rn("owner"),
            SubjectRef::userset(nn("group"), CanonicalId::new("eng").unwrap(), rn("member")),
        ),
    ]);
    assert!(allowed(&e, obj("document", "readme"), "owner", user("alice")).await);
    assert!(!allowed(&e, obj("document", "readme"), "owner", user("mallory")).await);
}

#[tokio::test]
async fn parallel_and_sequential_fanout_reach_the_same_decision() {
    // A wide fan-out: alice is in only the third sub-group, and the decision must be the same
    // whether the fan-out runs in parallel or sequentially.
    let userset =
        |id: &str| SubjectRef::userset(nn("group"), CanonicalId::new(id).unwrap(), rn("member"));
    let tuples = vec![
        RelationTuple::new(obj("document", "readme"), rn("owner"), userset("eng")),
        RelationTuple::new(obj("group", "eng"), rn("member"), userset("a")),
        RelationTuple::new(obj("group", "eng"), rn("member"), userset("b")),
        RelationTuple::new(obj("group", "eng"), rn("member"), userset("c")),
        tuple(obj("group", "c"), "member", user("alice")),
    ];
    for parallel in [true, false] {
        let engine = Engine::with_limits(
            MemoryStore::new(tuples.clone()),
            schema(),
            EvalLimits {
                parallel_fanout: parallel,
                ..EvalLimits::default()
            },
        );
        assert!(
            allowed(&engine, obj("document", "readme"), "owner", user("alice")).await,
            "alice is reachable via group c (parallel_fanout = {parallel})"
        );
        assert!(
            !allowed(&engine, obj("document", "readme"), "owner", user("mallory")).await,
            "mallory belongs to no group (parallel_fanout = {parallel})"
        );
    }
}

#[tokio::test]
async fn tuple_to_userset_walks_parent_folder() {
    let e = engine(vec![
        tuple(
            obj("document", "readme"),
            "parent",
            SubjectRef::leaf(nn("folder"), CanonicalId::new("root").unwrap()),
        ),
        tuple(obj("folder", "root"), "viewer", user("dave")),
    ]);
    assert!(allowed(&e, obj("document", "readme"), "viewer", user("dave")).await);
}

#[tokio::test]
async fn exclusion_removes_banned_from_viewers() {
    let e = engine(vec![
        tuple(obj("document", "readme"), "editor", user("carol")),
        tuple(obj("document", "readme"), "editor", user("bob")),
        tuple(obj("document", "readme"), "banned", user("carol")),
    ]);
    assert!(!allowed(&e, obj("document", "readme"), "viewer", user("carol")).await);
    assert!(allowed(&e, obj("document", "readme"), "viewer", user("bob")).await);
}

#[tokio::test]
async fn public_wildcard_grants_every_subject() {
    let e = engine(vec![tuple(
        obj("document", "pub"),
        "viewer",
        SubjectRef::wildcard(nn("user")),
    )]);
    assert!(allowed(&e, obj("document", "pub"), "viewer", user("anyone")).await);
    assert!(allowed(&e, obj("document", "pub"), "viewer", user("someone_else")).await);
}

#[tokio::test]
async fn a_caveat_reads_now_from_the_evaluations_clock() {
    let caveated =
        tuple(obj("document", "secret"), "editor", user("erin")).with_caveat(Caveat::new(
            cn("not_expired"),
            CaveatContext::new().with("valid_until", CaveatValue::Timestamp(SHIFT_UNTIL)),
        ));
    let e = engine(vec![caveated]);
    async fn check(e: &Engine<MemoryStore>) -> Result<bool, AuthzError> {
        e.check(
            obj("document", "secret"),
            rn("editor"),
            user("erin"),
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
    }

    e.store().set_now(at(SHIFT_UNTIL - 1));
    assert!(check(&e).await.expect("check"), "before valid_until");
    e.store().set_now(at(SHIFT_UNTIL));
    assert!(!check(&e).await.expect("check"), "at valid_until");
}

#[tokio::test]
async fn a_window_opens_at_from_and_closes_at_until_to_the_nanosecond() {
    let e = engine(vec![on_shift(the_shift())]);
    let cases = [
        (at(SHIFT_FROM - 1), false),
        (
            WallTime::from_unix_nanos(at(SHIFT_FROM).unix_nanos() - 1),
            false,
        ),
        (at(SHIFT_FROM), true),
        (
            WallTime::from_unix_nanos(at(SHIFT_UNTIL).unix_nanos() - 1),
            true,
        ),
        (at(SHIFT_UNTIL), false),
    ];
    for (now, admits) in cases {
        e.store().set_now(now);
        assert_eq!(
            erin_may_edit(&e, CaveatContext::new())
                .await
                .expect("check"),
            admits,
            "at {now:?}"
        );
    }
}

#[tokio::test]
async fn a_caller_cannot_supply_now() {
    let e = engine(vec![on_shift(the_shift())]);
    e.store().set_now(at(SHIFT_UNTIL + 60));
    let refused = erin_may_edit(
        &e,
        CaveatContext::new().with("now", CaveatValue::Timestamp(SHIFT_FROM + 60)),
    )
    .await
    .expect_err("a caller-supplied now must be refused");
    assert!(matches!(
        &refused,
        AuthzError::RequestCaveatParameter { parameter, count: 1 } if parameter == "now"
    ));
    assert_eq!(refused.error_code(), "AUTHZ_019");
    assert_eq!(refused.http_status_code(), 400);
    assert!(
        !erin_may_edit(&e, CaveatContext::new())
            .await
            .expect("check")
    );
}

#[tokio::test]
async fn a_stored_argument_wins_over_the_request() {
    let e = engine(vec![on_shift(the_shift())]);
    e.store().set_now(at(SHIFT_UNTIL + 60));
    let refused = erin_may_edit(
        &e,
        CaveatContext::new().with("until", CaveatValue::Timestamp(SHIFT_UNTIL + 3600)),
    )
    .await
    .expect_err("a caller naming a stored argument must be refused");
    assert!(matches!(
        &refused,
        AuthzError::RequestCaveatParameter { parameter, .. } if parameter == "until"
    ));
    assert!(
        !erin_may_edit(&e, CaveatContext::new())
            .await
            .expect("check"),
        "the stored until stands"
    );
}

#[tokio::test]
async fn a_stored_argument_the_tuple_left_out_cannot_be_filled_by_the_caller() {
    let open_ended =
        on_shift(CaveatContext::new().with("from", CaveatValue::Timestamp(SHIFT_FROM)));
    let e = engine(vec![open_ended]);
    e.store().set_now(at(SHIFT_FROM + 60));
    let refused = erin_may_edit(
        &e,
        CaveatContext::new().with("until", CaveatValue::Timestamp(i64::MAX)),
    )
    .await
    .expect_err("a caller filling an argument must be refused");
    assert_eq!(refused.error_code(), "AUTHZ_019");
    let unevaluable = erin_may_edit(&e, CaveatContext::new())
        .await
        .expect_err("a missing stored argument is an error");
    assert!(matches!(unevaluable, AuthzError::CaveatEval(_)));
}

#[tokio::test]
async fn a_stored_now_is_the_engines() {
    let frozen = on_shift(the_shift().with("now", CaveatValue::Timestamp(SHIFT_FROM + 60)));
    let e = engine(vec![frozen]);
    e.store().set_now(at(SHIFT_UNTIL + 60));
    assert!(
        !erin_may_edit(&e, CaveatContext::new())
            .await
            .expect("check")
    );
    e.store().set_now(at(SHIFT_FROM + 60));
    assert!(
        erin_may_edit(&e, CaveatContext::new())
            .await
            .expect("check")
    );
}

#[tokio::test]
async fn an_epoch_clock_is_refused() {
    let e = engine(vec![on_shift(the_shift())]);
    e.store().set_now(WallTime::from_unix_nanos(0));
    let refused = erin_may_edit(&e, CaveatContext::new())
        .await
        .expect_err("an evaluation at the epoch must be refused");
    assert!(matches!(
        refused,
        AuthzError::ImplausibleClock {
            unix_seconds: 0,
            floor: 1_767_225_600
        }
    ));
    assert_eq!(refused.error_code(), "AUTHZ_020");
    assert_eq!(refused.http_status_code(), 500);
    assert!(refused.is_retryable());

    e.store().set_now(at(1_767_225_600));
    assert!(
        !erin_may_edit(&e, CaveatContext::new())
            .await
            .expect("check")
    );
}

#[tokio::test]
async fn an_expired_window_leaves_expand() {
    use fundamental_authz::engine::UsersetTree;
    let e = engine(vec![
        on_shift(the_shift()),
        tuple(obj("document", "rota"), "editor", user("bob")),
    ]);
    async fn editors(e: &Engine<MemoryStore>) -> Result<UsersetTree, AuthzError> {
        e.expand(
            obj("document", "rota"),
            rn("editor"),
            Consistency::default(),
        )
        .await
    }

    e.store().set_now(at(SHIFT_FROM + 60));
    let during = editors(&e).await.expect("expand").all_subjects();
    assert!(during.contains(&user("erin")) && during.contains(&user("bob")));

    e.store().set_now(at(SHIFT_UNTIL));
    let after = editors(&e).await.expect("expand");
    let after_subjects = after.all_subjects();
    assert!(!after_subjects.contains(&user("erin")), "{after:?}");
    assert!(after_subjects.contains(&user("bob")));
    assert!(matches!(after, UsersetTree::Union(_)));
}

#[tokio::test]
async fn an_expired_window_leaves_the_parent_walk_of_expand() {
    let parent = RelationTuple::new(
        obj("document", "rota"),
        rn("parent"),
        SubjectRef::leaf(nn("folder"), CanonicalId::new("shared").unwrap()),
    )
    .with_caveat(Caveat::new(cn("on_duty"), the_shift()));
    let e = engine(vec![
        parent,
        tuple(obj("folder", "shared"), "owner", user("carol")),
    ]);
    async fn viewers(
        e: &Engine<MemoryStore>,
    ) -> Result<fundamental_authz::engine::UsersetTree, AuthzError> {
        e.expand(
            obj("document", "rota"),
            rn("viewer"),
            Consistency::default(),
        )
        .await
    }
    e.store().set_now(at(SHIFT_FROM + 60));
    assert!(
        viewers(&e)
            .await
            .expect("expand")
            .all_subjects()
            .contains(&user("carol"))
    );
    e.store().set_now(at(SHIFT_UNTIL));
    assert!(
        !viewers(&e)
            .await
            .expect("expand")
            .all_subjects()
            .contains(&user("carol"))
    );
}

#[tokio::test]
async fn list_objects_refuses_a_request_binding_and_honours_the_window() {
    let e = engine(vec![on_shift(the_shift())]);
    e.store().set_now(at(SHIFT_UNTIL + 60));
    async fn list(
        e: &Engine<MemoryStore>,
        context: CaveatContext,
    ) -> Result<Vec<ObjectRef>, AuthzError> {
        e.list_objects(
            user("erin"),
            nn("document"),
            rn("editor"),
            context,
            Consistency::default(),
            100,
        )
        .await
    }
    let refused = list(
        &e,
        CaveatContext::new().with("now", CaveatValue::Timestamp(SHIFT_FROM + 60)),
    )
    .await
    .expect_err("refused");
    assert_eq!(refused.error_code(), "AUTHZ_019");
    assert!(
        list(&e, CaveatContext::new())
            .await
            .expect("list")
            .is_empty()
    );
    e.store().set_now(at(SHIFT_FROM + 60));
    assert_eq!(
        list(&e, CaveatContext::new()).await.expect("list"),
        vec![obj("document", "rota")]
    );
}

#[tokio::test]
async fn cyclic_userset_terminates_without_hanging() {
    let self_ref = RelationTuple::new(
        obj("group", "loop"),
        rn("member"),
        SubjectRef::userset(nn("group"), CanonicalId::new("loop").unwrap(), rn("member")),
    );
    let e = engine(vec![self_ref]);
    // No real member ⇒ false, and the cycle terminates.
    assert!(!allowed(&e, obj("group", "loop"), "member", user("nobody")).await);
}

#[tokio::test]
async fn depth_limit_is_an_error_not_a_denial() {
    let e = Engine::with_limits(
        MemoryStore::new(vec![tuple(obj("document", "readme"), "owner", user("bob"))]),
        schema(),
        EvalLimits {
            max_depth: 1,
            ..EvalLimits::default()
        },
    );
    let err = e
        .check(
            obj("document", "readme"),
            rn("viewer"),
            user("bob"),
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
        .expect_err("must exceed depth");
    assert!(matches!(err, AuthzError::DepthExceeded { depth: 1 }));
}

#[tokio::test]
async fn read_budget_is_an_error_not_a_denial() {
    let e = Engine::with_limits(
        MemoryStore::new(vec![]),
        schema(),
        EvalLimits {
            max_reads: 1,
            ..EvalLimits::default()
        },
    );
    let err = e
        .check(
            obj("document", "x"),
            rn("owner"),
            user("absent"),
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
        .expect_err("must exceed budget");
    assert!(matches!(err, AuthzError::BudgetExceeded { budget: 1 }));
}

#[tokio::test]
async fn unknown_relation_is_an_error() {
    let e = engine(vec![]);
    let err = e
        .check(
            obj("document", "x"),
            rn("ghost"),
            user("a"),
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
        .expect_err("unknown relation");
    assert!(matches!(err, AuthzError::UndefinedRelation { .. }));
}

#[tokio::test]
async fn unknown_namespace_is_an_error() {
    let e = engine(vec![]);
    let err = e
        .check(
            obj("widget", "x"),
            rn("owner"),
            user("a"),
            CaveatContext::new(),
            Consistency::default(),
        )
        .await
        .expect_err("unknown namespace");
    assert!(matches!(err, AuthzError::UndefinedNamespace { .. }));
}

#[tokio::test]
async fn expand_returns_the_userset_tree() {
    use fundamental_authz::engine::UsersetTree;
    let e = engine(vec![
        tuple(obj("document", "readme"), "owner", user("bob")),
        RelationTuple::new(
            obj("document", "readme"),
            rn("owner"),
            SubjectRef::userset(nn("group"), CanonicalId::new("eng").unwrap(), rn("member")),
        ),
    ]);
    let tree = e
        .expand(
            obj("document", "readme"),
            rn("owner"),
            Consistency::default(),
        )
        .await
        .expect("expand");
    match tree {
        UsersetTree::Leaf(subjects) => {
            assert_eq!(subjects.len(), 2);
            assert!(subjects.iter().any(|s| s == &user("bob")));
            assert!(subjects.iter().any(SubjectRef::is_userset));
        }
        other => panic!("expected leaf, got {other:?}"),
    }

    let editor = e
        .expand(
            obj("document", "readme"),
            rn("editor"),
            Consistency::default(),
        )
        .await
        .expect("expand");
    assert!(matches!(editor, UsersetTree::Union(_)));

    let viewer = e
        .expand(
            obj("document", "readme"),
            rn("viewer"),
            Consistency::default(),
        )
        .await
        .expect("expand");
    assert!(matches!(viewer, UsersetTree::Exclusion { .. }));
}

#[tokio::test]
async fn list_objects_reverse_then_forward_confirms() {
    let e = engine(vec![
        tuple(obj("document", "a"), "owner", user("bob")),
        tuple(obj("document", "b"), "editor", user("bob")),
        tuple(obj("document", "c"), "editor", user("bob")),
        tuple(obj("document", "c"), "banned", user("bob")),
        tuple(obj("folder", "f"), "owner", user("bob")), // different namespace, excluded
    ]);

    let mut viewers = e
        .list_objects(
            user("bob"),
            nn("document"),
            rn("viewer"),
            CaveatContext::new(),
            Consistency::default(),
            100,
        )
        .await
        .expect("list");
    viewers.sort_by_key(|o| o.to_string());
    assert_eq!(viewers, vec![obj("document", "a"), obj("document", "b")]);
}

#[tokio::test]
async fn list_objects_resolves_indirect_membership() {
    let e = engine(vec![
        tuple(obj("group", "eng"), "member", user("alice")),
        RelationTuple::new(
            obj("document", "readme"),
            rn("owner"),
            SubjectRef::userset(nn("group"), CanonicalId::new("eng").unwrap(), rn("member")),
        ),
    ]);
    let owned = e
        .list_objects(
            user("alice"),
            nn("document"),
            rn("owner"),
            CaveatContext::new(),
            Consistency::default(),
            100,
        )
        .await
        .expect("list");
    assert_eq!(owned, vec![obj("document", "readme")]);
}

fn nested_membership() -> Vec<RelationTuple> {
    vec![
        RelationTuple::new(
            obj("group", "eng"),
            rn("member"),
            SubjectRef::userset(
                nn("group"),
                CanonicalId::new("platform").unwrap(),
                rn("member"),
            ),
        ),
        tuple(obj("group", "platform"), "member", user("alice")),
    ]
}

#[tokio::test]
async fn set_index_short_circuits_membership_without_touching_the_store() {
    let tuples = nested_membership();
    let index = Arc::new(TransitiveClosureIndex::build(
        &schema(),
        &tuples,
        HybridClock::now(),
    ));
    // The store is empty, so an allow proves the index answered alone.
    let engine = Engine::new(MemoryStore::new(vec![]), schema()).with_set_index(index);
    assert!(
        allowed(&engine, obj("group", "eng"), "member", user("alice")).await,
        "the index must answer alice's transitive membership"
    );
    assert!(!allowed(&engine, obj("group", "eng"), "member", user("mallory")).await);
}

#[tokio::test]
async fn set_index_agrees_with_the_authoritative_engine() {
    let tuples = nested_membership();
    let index = Arc::new(TransitiveClosureIndex::build(
        &schema(),
        &tuples,
        HybridClock::now(),
    ));
    let authoritative = engine(tuples.clone());
    let indexed = Engine::new(MemoryStore::new(tuples), schema()).with_set_index(index);
    for subject in [user("alice"), user("mallory")] {
        let want = allowed(
            &authoritative,
            obj("group", "eng"),
            "member",
            subject.clone(),
        )
        .await;
        let got = allowed(&indexed, obj("group", "eng"), "member", subject.clone()).await;
        assert_eq!(want, got, "indexed and authoritative must agree");
    }
}

#[tokio::test]
async fn a_fresher_than_index_request_bypasses_the_index() {
    let tuples = nested_membership();
    let watermark = HybridClock::now();
    let index = Arc::new(TransitiveClosureIndex::build(&schema(), &tuples, watermark));
    let engine = Engine::new(MemoryStore::new(vec![]), schema()).with_set_index(index);

    // A read fresher than the index watermark falls back to the empty store.
    let future = Zookie::new(
        BackendTag::Offline,
        HybridTs::new(u64::MAX, 0),
        schema().hash(),
    );
    let allowed = engine
        .check(
            obj("group", "eng"),
            rn("member"),
            user("alice"),
            CaveatContext::new(),
            Consistency::AtLeastAsFresh(future),
        )
        .await
        .expect("check");
    assert!(
        !allowed,
        "a fresher-than-watermark request must bypass the stale index"
    );
}

#[tokio::test]
async fn a_request_binding_is_refused_before_the_set_index_answers() {
    let tuples = nested_membership();
    let index = Arc::new(TransitiveClosureIndex::build(
        &schema(),
        &tuples,
        HybridClock::now(),
    ));
    let engine = Engine::new(MemoryStore::new(vec![]), schema()).with_set_index(index);
    let refused = engine
        .check(
            obj("group", "eng"),
            rn("member"),
            user("alice"),
            CaveatContext::new().with("now", CaveatValue::Timestamp(SHIFT_FROM)),
            Consistency::default(),
        )
        .await
        .expect_err("refused even where the index would answer");
    assert_eq!(refused.error_code(), "AUTHZ_019");
}
