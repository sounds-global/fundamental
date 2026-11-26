//! Engine-scaling benchmark: a `check` walk over nested groups by depth and by width, over a
//! store that sleeps per read so storage round-trips dominate.
#![allow(dead_code)] // the `Group::Member` variant is a schema marker, never constructed
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fundamental_authz::AuthzError;
use fundamental_authz::engine::{Engine, TransitiveClosureIndex};
use fundamental_authz::schema::{NamespaceName, RelationName, SchemaIr};
use std::sync::Arc;

use fundamental_authz::tuple::{
    CanonicalId, ClockScope, DirectEdge, ObjectRef, RelationTuple, ScopedEval, SubjectRef,
    TupleStore,
};
use fundamental_authz::zookie::{BackendTag, Consistency, HybridClock, HybridTs};
use fundamental_macros::Namespace;
use tokio::runtime::Runtime;

#[derive(Namespace)]
enum User {}

/// One namespace whose `member` relation admits users and other groups' members.
#[derive(Namespace)]
enum Group {
    #[relation(direct(User, Group::Member))]
    Member,
}

/// An in-memory store that sleeps `latency` before serving each read.
struct LatencyStore {
    tuples: Vec<RelationTuple>,
    latency: Duration,
}

impl LatencyStore {
    fn new(tuples: Vec<RelationTuple>, latency: Duration) -> Self {
        Self { tuples, latency }
    }

    async fn delay(&self) {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
    }
}

impl TupleStore for LatencyStore {
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
        body(Arc::new(ClockScope::current())).await
    }

    async fn read_edge(
        &self,
        _scope: &Self::Scope,
        object: &ObjectRef,
        relation: &RelationName,
        subject: &SubjectRef,
    ) -> Result<Option<DirectEdge>, AuthzError> {
        self.delay().await;
        Ok(self
            .tuples
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
        self.delay().await;
        Ok(self
            .tuples
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
        self.delay().await;
        Ok(self
            .tuples
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
        self.delay().await;
        Ok(self
            .tuples
            .iter()
            .filter(|t| &t.subject == subject)
            .filter(|t| object_namespace.is_none_or(|ns| &t.object.namespace == ns))
            .filter(|t| relation.is_none_or(|r| &t.relation == r))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn write(&self, _tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        Ok(HybridClock::now())
    }

    async fn delete(&self, _tuples: &[RelationTuple]) -> Result<HybridTs, AuthzError> {
        Ok(HybridClock::now())
    }
}

fn group(id: &str) -> ObjectRef {
    ObjectRef::new(
        NamespaceName::new("group").unwrap(),
        CanonicalId::new(id).unwrap(),
    )
}

fn member() -> RelationName {
    RelationName::new("member").unwrap()
}

fn alice() -> SubjectRef {
    SubjectRef::leaf(
        NamespaceName::new("user").unwrap(),
        CanonicalId::new("alice").unwrap(),
    )
}

fn group_member(id: &str) -> SubjectRef {
    SubjectRef::userset(
        NamespaceName::new("group").unwrap(),
        CanonicalId::new(id).unwrap(),
        member(),
    )
}

fn schema() -> std::sync::Arc<SchemaIr> {
    std::sync::Arc::new(
        SchemaIr::builder()
            .namespace::<User>()
            .namespace::<Group>()
            .build()
            .expect("valid schema"),
    )
}

/// A chain `g0 ⊃ user:alice`, `g{i+1} ⊃ g{i}#member`.
fn depth_chain(depth: usize) -> Vec<RelationTuple> {
    let mut tuples = vec![RelationTuple::new(group("g0"), member(), alice())];
    for i in 0..depth {
        tuples.push(RelationTuple::new(
            group(&format!("g{}", i + 1)),
            member(),
            group_member(&format!("g{i}")),
        ));
    }
    tuples
}

/// One group `top` with `width` child groups, only the last containing `alice`.
fn width_fanout(width: usize) -> Vec<RelationTuple> {
    let mut tuples = Vec::with_capacity(width + 1);
    for i in 0..width {
        tuples.push(RelationTuple::new(
            group("top"),
            member(),
            group_member(&format!("c{i}")),
        ));
    }
    tuples.push(RelationTuple::new(
        group(&format!("c{}", width - 1)),
        member(),
        alice(),
    ));
    tuples
}

/// Per-read latency, about one storage round-trip.
const READ_LATENCY: Duration = Duration::from_millis(1);

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn bench_depth(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runtime");
    let schema = schema();
    let mut bench_group = c.benchmark_group("check/depth");
    for &depth in &[1usize, 8, 32] {
        let store = LatencyStore::new(depth_chain(depth), READ_LATENCY);
        let engine = Engine::new(store, schema.clone());
        let object = group(&format!("g{depth}"));
        bench_group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, _| {
            b.iter(|| {
                runtime.block_on(async {
                    let allowed = engine
                        .check(
                            black_box(object.clone()),
                            member(),
                            alice(),
                            Default::default(),
                            Consistency::default(),
                        )
                        .await
                        .expect("check");
                    black_box(allowed)
                })
            });
        });
    }
    bench_group.finish();
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn bench_width(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runtime");
    let schema = schema();
    let mut bench_group = c.benchmark_group("check/width");
    for &width in &[1usize, 8, 32] {
        let store = LatencyStore::new(width_fanout(width), READ_LATENCY);
        let engine = Engine::new(store, schema.clone());
        bench_group.bench_with_input(BenchmarkId::from_parameter(width), &width, |b, _| {
            b.iter(|| {
                runtime.block_on(async {
                    let allowed = engine
                        .check(
                            black_box(group("top")),
                            member(),
                            alice(),
                            Default::default(),
                            Consistency::default(),
                        )
                        .await
                        .expect("check");
                    black_box(allowed)
                })
            });
        });
    }
    bench_group.finish();
}

#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn bench_depth_indexed(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runtime");
    let schema = schema();
    let mut bench_group = c.benchmark_group("check/depth-indexed");
    for &depth in &[1usize, 8, 32] {
        let tuples = depth_chain(depth);
        // The index precomputes the membership chain, so a check is one lookup at any depth.
        let index = Arc::new(TransitiveClosureIndex::build(
            &schema,
            &tuples,
            HybridClock::now(),
        ));
        let store = LatencyStore::new(tuples, READ_LATENCY);
        let engine = Engine::new(store, schema.clone()).with_set_index(index);
        let object = group(&format!("g{depth}"));
        bench_group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, _| {
            b.iter(|| {
                runtime.block_on(async {
                    let allowed = engine
                        .check(
                            black_box(object.clone()),
                            member(),
                            alice(),
                            Default::default(),
                            Consistency::default(),
                        )
                        .await
                        .expect("check");
                    black_box(allowed)
                })
            });
        });
    }
    bench_group.finish();
}

fn bench_direct(c: &mut Criterion) {
    let runtime = Runtime::new().expect("runtime");
    let schema = schema();
    let store = LatencyStore::new(
        vec![RelationTuple::new(group("g0"), member(), alice())],
        READ_LATENCY,
    );
    let engine = Engine::new(store, schema);
    c.bench_function("check/direct", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let allowed = engine
                    .check(
                        black_box(group("g0")),
                        member(),
                        alice(),
                        Default::default(),
                        Consistency::default(),
                    )
                    .await
                    .expect("check");
                black_box(allowed)
            })
        });
    });
}

criterion_group!(
    benches,
    bench_direct,
    bench_depth,
    bench_depth_indexed,
    bench_width
);
criterion_main!(benches);
