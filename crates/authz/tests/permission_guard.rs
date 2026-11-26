//! The `authorize(...)` permission middleware end to end, over the offline store.
#![cfg(all(feature = "server", feature = "offline", not(target_arch = "wasm32")))]
#![allow(missing_docs, dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use fundamental_auth::{AuthenticatedPrincipal, UserId};
use fundamental_authz::AuthzError;
use fundamental_authz::authorizer::{Authorizer, DynAuthorizer, HasAuthorizer, NativeEngine};
use fundamental_authz::backend::OfflineTupleStore;
use fundamental_authz::middleware::authorize;
use fundamental_authz::schema::{
    CaveatIr, CaveatName, CaveatOperand, CaveatValue, CompareOp, Namespace, NamespaceExt,
    NamespaceName, RelationName, SchemaIr,
};
use fundamental_authz::tuple::CaveatContext;
use fundamental_authz::tuple::{
    CanonicalId, Caveat, ClockScope, DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef,
    TupleStore, WallTime,
};
use fundamental_authz::zookie::{BackendTag, Consistency, HybridClock, HybridTs};
use fundamental_core::AnyError;
use fundamental_macros::Namespace;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_server::{HttpContext, PathParams, Route};
use tracing_test::traced_test;

#[derive(Namespace)]
enum User {}

#[derive(Namespace)]
enum Document {
    #[relation(direct(User))]
    Owner,
    #[relation(union(Owner, direct(User)))]
    Viewer,
}

#[derive(Clone)]
struct AppState {
    authz: Arc<dyn DynAuthorizer>,
    /// A configured value a route cannot know, reached by `object_from_state`.
    tenant_document: String,
}

impl HasAuthorizer for AppState {
    fn authorizer(&self) -> &dyn DynAuthorizer {
        self.authz.as_ref()
    }
}

fn schema() -> Arc<SchemaIr> {
    Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace::<Document>()
            .namespace::<Cased>()
            .build()
            .expect("valid schema"),
    )
}

fn tuple(object: &str, relation: &str, subject: SubjectRef) -> RelationTuple {
    RelationTuple::new(
        ObjectRef::new(
            NamespaceName::new("document").unwrap(),
            CanonicalId::new(object).unwrap(),
        ),
        RelationName::new(relation).unwrap(),
        subject,
    )
}

fn user(id: &str) -> SubjectRef {
    SubjectRef::leaf(
        NamespaceName::new("user").unwrap(),
        CanonicalId::new(id).unwrap(),
    )
}

fn everyone() -> SubjectRef {
    SubjectRef::wildcard(NamespaceName::new("user").unwrap())
}

async fn app_state_with(tuples: &[RelationTuple]) -> AppState {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");
    let authz: Arc<dyn DynAuthorizer> = Arc::new(NativeEngine::new(store, schema()));
    authz.write(tuples).await.expect("write");
    AppState {
        authz,
        tenant_document: "readme".to_string(),
    }
}

async fn app_state() -> AppState {
    app_state_with(&[tuple("readme", "owner", user("alice"))]).await
}

fn principal(user: &str) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        user_id: UserId::new(user),
        device_id: None,
        scopes: Vec::new(),
        expires_at: 0,
    }
}

/// A namespace whose ids have one spelling; deliberately not a UUID, to test the seam.
#[derive(Namespace)]
#[namespace(canonical_id = lower_case_id)]
enum Cased {
    #[relation(direct(User))]
    Viewer,
}

fn lower_case_id(raw: &str) -> Option<std::borrow::Cow<'_, str>> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if raw.bytes().any(|b| b.is_ascii_uppercase()) {
        return Some(std::borrow::Cow::Owned(raw.to_ascii_lowercase()));
    }
    Some(std::borrow::Cow::Borrowed(raw))
}

fn cased_route() -> Route<AppState> {
    Route::get("/cased/{id}", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Cased::Viewer))
}

fn cased_context(state: AppState, user: &str, id: &str) -> HttpContext<AppState> {
    HttpContext::new(state, "req-1")
        .with_params(PathParams::from_iter([("id".to_string(), id.to_string())]))
        .with_principal(Some(principal(user)))
}

fn guarded_route() -> Route<AppState> {
    Route::get("/documents/{id}", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer))
}

fn context(state: AppState, user: Option<&str>) -> HttpContext<AppState> {
    let ctx = HttpContext::new(state, "req-1").with_params(PathParams::from_iter([(
        "id".to_string(),
        "readme".to_string(),
    )]));
    match user {
        Some(u) => ctx.with_principal(Some(principal(u))),
        None => ctx,
    }
}

#[tokio::test]
async fn allows_a_principal_that_holds_the_relation() {
    let state = app_state().await;
    let body = guarded_route()
        .dispatch(context(state, Some("alice")), Vec::new())
        .await
        .expect("guard allows alice");
    assert_eq!(body, b"null"); // the unit handler serializes to JSON null
}

#[tokio::test]
async fn denies_a_principal_without_the_relation() {
    let state = app_state().await;
    let err = guarded_route()
        .dispatch(context(state, Some("bob")), Vec::new())
        .await
        .expect_err("guard denies bob");
    assert_eq!(err.get().http_status_code(), 403);
    assert_eq!(err.get().error_code(), "AUTHZ_014");
}

/// An anonymous caller with no wildcard grant gets `401`, not `403`.
#[tokio::test]
async fn rejects_an_unauthenticated_request() {
    let state = app_state().await;
    let err = guarded_route()
        .dispatch(context(state, None), Vec::new())
        .await
        .expect_err("no principal");
    assert_eq!(err.get().http_status_code(), 401);
}

// ── The guard can express "public" ──────────────────────────────────────────────

/// A `user:*` grant is reachable through the middleware for an anonymous request.
#[tokio::test]
async fn a_public_wildcard_grant_admits_an_anonymous_request() {
    let state = app_state_with(&[tuple("readme", "viewer", everyone())]).await;
    let body = guarded_route()
        .dispatch(context(state, None), Vec::new())
        .await
        .expect("a wildcard grant is reachable without a principal");
    assert_eq!(body, b"null");
}

/// The wildcard admits every subject of the namespace, named or not.
#[tokio::test]
async fn a_public_wildcard_grant_admits_a_named_principal_too() {
    let state = app_state_with(&[tuple("readme", "viewer", everyone())]).await;
    let body = guarded_route()
        .dispatch(context(state, Some("nobody-in-particular")), Vec::new())
        .await
        .expect("wildcard covers concrete leaves");
    assert_eq!(body, b"null");
}

/// A wildcard on one object must not leak to another.
#[tokio::test]
async fn a_wildcard_on_another_object_does_not_admit_this_one() {
    let state = app_state_with(&[tuple("secrets", "viewer", everyone())]).await;
    let err = guarded_route()
        .dispatch(context(state, None), Vec::new())
        .await
        .expect_err("readme is not public");
    assert_eq!(err.get().http_status_code(), 401);
}

// ── The object need not be a path parameter ───────────────────────────────────

/// A route asking for a path parameter it does not have is a `500`.
#[tokio::test]
async fn a_route_with_no_path_parameter_reports_the_misconfiguration() {
    let state = app_state().await;
    let route = Route::get("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer));
    let err = route
        .dispatch(HttpContext::new(state, "req-1"), Vec::new())
        .await
        .expect_err("no object to authorize");
    assert_eq!(err.get().http_status_code(), 500);
    assert_eq!(err.get().error_code(), "AUTHZ_015");
}

/// Two path parameters are ambiguous, so the guard refuses rather than guessing.
#[tokio::test]
async fn two_path_parameters_are_ambiguous_unless_one_is_named() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1")
        .with_params(PathParams::from_iter([
            ("id".to_string(), "readme".to_string()),
            ("revision".to_string(), "7".to_string()),
        ]))
        .with_principal(Some(principal("alice")));
    let err = Route::get(
        "/documents/{id}/revisions/{revision}",
        |_ctx, _input: ()| async move { Ok::<_, AnyError>(()) },
    )
    .middleware(authorize(Document::Viewer))
    .dispatch(ctx, Vec::new())
    .await
    .expect_err("ambiguous object");
    assert_eq!(err.get().error_code(), "AUTHZ_015");
}

#[tokio::test]
async fn a_named_path_parameter_resolves_the_object() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1")
        .with_params(PathParams::from_iter([
            ("id".to_string(), "readme".to_string()),
            ("revision".to_string(), "7".to_string()),
        ]))
        .with_principal(Some(principal("alice")));
    let body = Route::get(
        "/documents/{id}/revisions/{revision}",
        |_ctx, _input: ()| async move { Ok::<_, AnyError>(()) },
    )
    .middleware(authorize(Document::Viewer).object("id"))
    .dispatch(ctx, Vec::new())
    .await
    .expect("named parameter resolves");
    assert_eq!(body, b"null");
}

/// A constant object: the route addresses an act, not a resource.
#[tokio::test]
async fn a_constant_object_needs_no_path_parameter() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1").with_principal(Some(principal("alice")));
    let body = Route::post("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer).object_const("readme"))
    .dispatch(ctx, Vec::new())
    .await
    .expect("constant object resolves");
    assert_eq!(body, b"null");
}

#[tokio::test]
async fn a_constant_object_is_still_checked() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1").with_principal(Some(principal("bob")));
    let err = Route::post("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer).object_const("readme"))
    .dispatch(ctx, Vec::new())
    .await
    .expect_err("bob holds nothing");
    assert_eq!(err.get().error_code(), "AUTHZ_014");
}

/// A state-derived object, which a route declaration cannot know.
#[tokio::test]
async fn a_state_derived_object_resolves_from_the_app_state() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1").with_principal(Some(principal("alice")));
    let body = Route::post("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(
        authorize(Document::Viewer)
            .object_from_state(|state: &AppState| Some(state.tenant_document.clone())),
    )
    .dispatch(ctx, Vec::new())
    .await
    .expect("state resolves the object");
    assert_eq!(body, b"null");
}

/// A state accessor that names nothing is a misconfiguration, not a denial.
#[tokio::test]
async fn a_state_accessor_that_names_nothing_reports_the_misconfiguration() {
    let state = app_state().await;
    let ctx = HttpContext::new(state, "req-1").with_principal(Some(principal("alice")));
    let err = Route::post("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer).object_from_state(|_state: &AppState| None))
    .dispatch(ctx, Vec::new())
    .await
    .expect_err("no object named");
    assert_eq!(err.get().error_code(), "AUTHZ_015");
}

// ── Every refusal is diagnosable from the log ───────────────────────────────────

/// Both refusal shapes name the object, relation and subject in the log.
#[tokio::test]
#[traced_test]
async fn a_denial_names_the_object_relation_and_subject_in_the_log() {
    let state = app_state().await;
    let _ = guarded_route()
        .dispatch(context(state, Some("bob")), Vec::new())
        .await
        .expect_err("guard denies bob");

    assert!(logs_contain("permission denied"));
    assert!(logs_contain("object_namespace=document"));
    assert!(logs_contain("object_id=readme"));
    assert!(logs_contain("relation=viewer"));
    assert!(logs_contain("subject=\"bob\""));
}

/// An anonymous denial still says what identified it.
#[tokio::test]
#[traced_test]
async fn an_anonymous_denial_is_logged_as_anonymous() {
    let state = app_state().await;
    let _ = guarded_route()
        .dispatch(context(state, None), Vec::new())
        .await
        .expect_err("no principal");

    assert!(logs_contain("permission denied"));
    assert!(logs_contain("subject=\"<anonymous>\""));
}

/// A misconfiguration names the source it asked.
#[tokio::test]
#[traced_test]
async fn a_missing_object_names_the_source_it_asked() {
    let state = app_state().await;
    let _ = Route::post("/documents", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer).object("nonexistent"))
    .dispatch(HttpContext::new(state, "req-1"), Vec::new())
    .await
    .expect_err("no object to authorize");

    assert!(logs_contain("authorization guard resolved no object id"));
    assert!(logs_contain("path parameter `nonexistent`"));
}

// ── Freshness: a security decision does not read stale by default ───────────────

/// A store that records the freshness each evaluation asked for, since the offline store
/// ignores consistency.
#[derive(Clone)]
struct RecordingStore {
    inner: OfflineTupleStore,
    asked: Arc<Mutex<Vec<Consistency>>>,
}

impl TupleStore for RecordingStore {
    fn backend(&self) -> BackendTag {
        self.inner.backend()
    }

    type Scope = ClockScope;

    /// Records the freshness asked for, then runs the body in the wrapped store's scope.
    async fn evaluate<'a, F, T>(
        &'a self,
        consistency: Consistency,
        body: F,
    ) -> Result<T, AuthzError>
    where
        F: Fn(Arc<Self::Scope>) -> ScopedEval<'a, T> + Send + Sync + 'a,
        T: Send + 'a,
    {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(consistency.clone());
        self.inner.evaluate(consistency, body).await
    }

    async fn read_edge(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        self.inner.read_edge(scope, object, relation, subject).await
    }

    async fn read_usersets(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner
            .read_usersets(scope, object, relation, limit)
            .await
    }

    async fn read_all(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner.read_all(scope, object, relation, limit).await
    }

    async fn read_by_subject(
        &self,
        scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner
            .read_by_subject(scope, subject, object_namespace, relation, limit)
            .await
    }

    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        self.inner.write(tuples).await
    }

    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        self.inner.delete(tuples).await
    }
}

async fn recording_state() -> (AppState, Arc<Mutex<Vec<Consistency>>>) {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let inner = OfflineTupleStore::new(db.into_shared());
    inner.ensure_schema().await.expect("schema");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let store = RecordingStore {
        inner,
        asked: Arc::clone(&asked),
    };
    let authz: Arc<dyn DynAuthorizer> = Arc::new(NativeEngine::new(store, schema()));
    authz
        .write(&[tuple("readme", "owner", user("alice"))])
        .await
        .expect("write");
    (
        AppState {
            authz,
            tenant_document: "readme".to_string(),
        },
        asked,
    )
}

/// A permission check reads at full consistency by default, against the "new enemy" problem.
#[tokio::test]
async fn a_permission_check_reads_the_freshest_snapshot_by_default() {
    let (state, asked) = recording_state().await;
    guarded_route()
        .dispatch(context(state, Some("alice")), Vec::new())
        .await
        .expect("allowed");

    let asked = asked.lock().expect("not poisoned");
    assert_eq!(asked.as_slice(), &[Consistency::FullConsistency]);
}

/// Speed is available, but only by saying so.
#[tokio::test]
async fn the_freshness_is_overridable_for_a_route_that_can_afford_staleness() {
    let (state, asked) = recording_state().await;
    Route::get("/documents/{id}", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Document::Viewer).consistency(Consistency::MinimizeLatency))
    .dispatch(context(state, Some("alice")), Vec::new())
    .await
    .expect("allowed");

    let asked = asked.lock().expect("not poisoned");
    assert_eq!(asked.as_slice(), &[Consistency::MinimizeLatency]);
}

// ── Subject namespace ───────────────────────────────────────────────────────────

/// A deployment whose principals are another type says so.
#[tokio::test]
async fn the_subject_namespace_is_overridable() {
    let state = app_state().await;
    let err = guarded_route()
        .middleware(authorize(Document::Viewer).subject_namespace("service"))
        .dispatch(context(state, Some("alice")), Vec::new())
        .await
        .expect_err("alice is a user, not a service");
    assert_eq!(err.get().error_code(), "AUTHZ_014");
}

// ---- the object id has one spelling ----------------------------------------

/// Two spellings of one id are one object: the guard applies the namespace's form first.
#[tokio::test]
async fn a_non_canonical_spelling_authorizes_as_the_object_it_names() {
    let state = app_state_with(&[RelationTuple::new(
        Cased::object("readme").expect("a valid object"),
        RelationName::new("viewer").expect("a valid relation"),
        user("alice"),
    )])
    .await;

    for spelling in ["readme", "README", "ReadMe"] {
        cased_route()
            .dispatch(cased_context(state.clone(), "alice", spelling), Vec::new())
            .await
            .unwrap_or_else(|error| panic!("{spelling} must authorize as `readme`: {error}"));
    }
}

/// A canonical form returning `None` denies, never passes the raw id through.
#[tokio::test]
async fn an_id_that_is_not_of_the_namespace_is_refused() {
    let state = app_state().await;
    let error = cased_route()
        .dispatch(cased_context(state, "alice", "not a valid id"), Vec::new())
        .await
        .expect_err("a malformed object id must not authorize");

    assert_eq!(error.get().error_code(), "AUTHZ_016");
    assert_eq!(
        error.get().http_status_code(),
        403,
        "the status must not distinguish a malformed id from an unheld relation"
    );
}

/// The writer uses the same form as the reader.
#[test]
fn the_writer_stores_the_canonical_spelling() {
    let object = Cased::object("README").expect("a valid object");
    assert_eq!(object.id.as_str(), "readme");
}

/// A namespace that declares nothing keeps opaque ids.
#[test]
fn a_namespace_that_declares_nothing_leaves_its_ids_alone() {
    let object = Document::object("ReadMe.MD").expect("a valid object");
    assert_eq!(object.id.as_str(), "ReadMe.MD");
}

/// The form governs subjects too.
#[test]
fn the_form_governs_subjects_as_well_as_objects() {
    let subject = Cased::subject("README").expect("a valid subject");
    assert_eq!(subject.object.id.as_str(), "readme");
}

/// The typed check answers the same question about the same id as the guard.
#[tokio::test]
async fn the_typed_check_reduces_the_spelling_the_guard_reduces() {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let store = OfflineTupleStore::new(db.into_shared());
    store.ensure_schema().await.expect("schema");
    let engine = NativeEngine::new(store, schema());
    Authorizer::write(
        &engine,
        &[RelationTuple::new(
            Cased::object("readme").expect("a valid object"),
            RelationName::new("viewer").expect("a valid relation"),
            user("alice"),
        )],
    )
    .await
    .expect("the grant is written");

    let response = engine
        .check_typed::<Cased>(
            Cased::Viewer,
            CanonicalId::new("README").expect("a valid id"),
            user("alice"),
            CaveatContext::new(),
            Consistency::FullConsistency,
        )
        .await
        .expect("the check runs");

    assert!(
        response.allowed,
        "`README` must be the object `readme` for the typed check, as it is for the guard"
    );
}

// ── Time-bounded grants: the guard judges a window at the evaluation's own clock ─

#[derive(Namespace)]
enum Team {
    #[relation(direct(User))]
    Operator,
}

/// 2026-10-01T09:00:00Z.
const SHIFT_FROM: u64 = 1_790_845_200;
const SHIFT_UNTIL: u64 = SHIFT_FROM + 8 * 3600;

fn team_schema() -> Arc<SchemaIr> {
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
            .namespace_ir(Team::ir().with_caveats(BTreeMap::from([(
                CaveatName::new("on_duty").expect("a valid caveat name"),
                on_duty,
            )])))
            .build()
            .expect("valid schema"),
    )
}

/// The offline store with a clock the test moves.
#[derive(Clone)]
struct ShiftClockStore {
    inner: OfflineTupleStore,
    now: Arc<Mutex<WallTime>>,
}

impl TupleStore for ShiftClockStore {
    fn backend(&self) -> BackendTag {
        self.inner.backend()
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
        let now = *self.now.lock().expect("not poisoned");
        body(Arc::new(ClockScope::new(HybridClock::now(), now))).await
    }

    async fn read_edge(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        self.inner.read_edge(scope, object, relation, subject).await
    }

    async fn read_usersets(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner
            .read_usersets(scope, object, relation, limit)
            .await
    }

    async fn read_all(
        &self,
        scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner.read_all(scope, object, relation, limit).await
    }

    async fn read_by_subject(
        &self,
        scope: &Self::Scope,
        subject: &SubjectRef,
        object_namespace: Option<&NamespaceName>,
        relation: Option<&RelationName>,
        limit: usize,
    ) -> Result<Vec<RelationTuple>, AuthzError> {
        self.inner
            .read_by_subject(scope, subject, object_namespace, relation, limit)
            .await
    }

    async fn write(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        self.inner.write(tuples).await
    }

    async fn delete(&self, tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        self.inner.delete(tuples).await
    }
}

/// A state granting `team:night#operator@user:olga` for the shift only, and its clock.
async fn shift_state() -> (AppState, Arc<Mutex<WallTime>>) {
    let db = OfflineDatabase::open(OfflineConfig::memory())
        .await
        .expect("open");
    let inner = OfflineTupleStore::new(db.into_shared());
    inner.ensure_schema().await.expect("schema");
    let now = Arc::new(Mutex::new(WallTime::from_unix_seconds(SHIFT_FROM)));
    let store = ShiftClockStore {
        inner,
        now: Arc::clone(&now),
    };
    let authz: Arc<dyn DynAuthorizer> = Arc::new(NativeEngine::new(store, team_schema()));
    let shift = Caveat::new(
        CaveatName::new("on_duty").expect("a valid caveat name"),
        CaveatContext::new()
            .with("from", CaveatValue::Timestamp(SHIFT_FROM.cast_signed()))
            .with("until", CaveatValue::Timestamp(SHIFT_UNTIL.cast_signed())),
    );
    authz
        .write(&[RelationTuple::new(
            Team::object("night").expect("a valid object"),
            RelationName::new("operator").expect("a valid relation"),
            user("olga"),
        )
        .with_caveat(shift)])
        .await
        .expect("write");
    (
        AppState {
            authz,
            tenant_document: "readme".to_string(),
        },
        now,
    )
}

fn team_route() -> Route<AppState> {
    Route::get("/teams/{id}", |_ctx, _input: ()| async move {
        Ok::<_, AnyError>(())
    })
    .middleware(authorize(Team::Operator))
}

fn olga_on(state: AppState, team: &str) -> HttpContext<AppState> {
    HttpContext::new(state, "req-1")
        .with_params(PathParams::from_iter([(
            "id".to_string(),
            team.to_string(),
        )]))
        .with_principal(Some(principal("olga")))
}

/// A windowed grant admits inside the window and is a `403` after it, never a `500`.
#[tokio::test]
async fn a_time_caveated_grant_admits_until_valid_until_then_denies() {
    let (state, now) = shift_state().await;
    let set = |at: WallTime| *now.lock().expect("not poisoned") = at;

    for (at, label) in [
        (
            WallTime::from_unix_seconds(SHIFT_FROM),
            "as the shift opens",
        ),
        (
            WallTime::from_unix_nanos(WallTime::from_unix_seconds(SHIFT_UNTIL).unix_nanos() - 1),
            "in the shift's last nanosecond",
        ),
    ] {
        set(at);
        team_route()
            .dispatch(olga_on(state.clone(), "night"), Vec::new())
            .await
            .unwrap_or_else(|err| panic!("olga is on duty {label}: {err:?}"));
    }

    for at in [SHIFT_UNTIL, SHIFT_FROM - 1] {
        set(WallTime::from_unix_seconds(at));
        let err = team_route()
            .dispatch(olga_on(state.clone(), "night"), Vec::new())
            .await
            .expect_err("olga is off duty");
        assert_eq!(err.get().error_code(), "AUTHZ_014", "at {at}");
        assert_eq!(err.get().http_status_code(), 403, "at {at}");
    }
}

/// What the guard allowed reaches the handler as a type; nothing the request carries can.
#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn the_handler_sees_the_object_the_guard_authorized() {
    use fundamental_authz::middleware::Authorized;

    let state = app_state().await;
    let seen = Arc::new(Mutex::new(None::<String>));
    let route = {
        let seen = Arc::clone(&seen);
        Route::get(
            "/documents/{id}",
            move |ctx: HttpContext<AppState>, _input: ()| {
                let seen = Arc::clone(&seen);
                async move {
                    *seen.lock().unwrap() =
                        Authorized::<Document>::from_context(&ctx).map(|a| a.id().to_owned());
                    Ok::<_, AnyError>(())
                }
            },
        )
        .middleware(authorize(Document::Viewer))
    };
    route
        .dispatch(context(state.clone(), Some("alice")), Vec::new())
        .await
        .expect("alice may view readme");
    assert_eq!(seen.lock().unwrap().as_deref(), Some("readme"));

    let unguarded = HttpContext::new(state, "req-2").with_params(PathParams::from_iter([(
        "id".to_string(),
        "readme".to_string(),
    )]));
    assert!(Authorized::<Document>::from_context(&unguarded).is_none());
    assert_eq!(authorize(Document::Viewer).object_namespace(), "document");
}
