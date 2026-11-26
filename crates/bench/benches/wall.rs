//! The workloads `benches/instructions.rs` counts, timed by criterion. Run with `fun bench wall`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use fundamental_bench::workloads::action::ActionDispatch;
use fundamental_bench::workloads::config::ConfigLoad;
use fundamental_bench::workloads::migration::Migration;
use fundamental_bench::workloads::sync::SyncWire;
use fundamental_bench::workloads::{mf2, query, render};

fn bench_render(c: &mut Criterion) {
    c.bench_function("render/page", |b| b.iter(|| black_box(render::page())));
}

fn bench_action(c: &mut Criterion) {
    let dispatch = ActionDispatch::new();
    c.bench_function("action/dispatch", |b| {
        b.iter(|| black_box(dispatch.dispatch()));
    });
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("query");
    group.bench_function("select_sql", |b| b.iter(|| black_box(query::select_sql())));
    group.bench_function("update_sql", |b| b.iter(|| black_box(query::update_sql())));
    group.bench_function("select_cql", |b| b.iter(|| black_box(query::select_cql())));
    group.finish();
}

fn bench_migration(c: &mut Criterion) {
    let migration = Migration::new();
    let mut group = c.benchmark_group("migration");
    group.bench_function("diff", |b| b.iter(|| black_box(migration.diff())));
    group.bench_function("generate", |b| b.iter(|| black_box(migration.generate())));
    group.bench_function("differences", |b| {
        b.iter(|| black_box(migration.differences()));
    });
    group.finish();
}

fn bench_sync(c: &mut Criterion) {
    let wire = SyncWire::new();
    let mut group = c.benchmark_group("sync");
    group.bench_function("encode_pull", |b| b.iter(|| black_box(wire.encode_pull())));
    group.bench_function("decode_pull", |b| b.iter(|| black_box(wire.decode_pull())));
    group.bench_function("push_round_trip", |b| {
        b.iter(|| black_box(wire.push_round_trip()));
    });
    group.bench_function("canonical", |b| b.iter(|| black_box(wire.canonical())));
    group.bench_function("envelope_round_trip", |b| {
        b.iter(|| black_box(wire.envelope_round_trip()));
    });
    group.finish();
}

#[expect(
    clippy::expect_used,
    reason = "a bench fixture; a failed write ends the run"
)]
fn bench_config(c: &mut Criterion) {
    let config = ConfigLoad::new().expect("a project directory");
    let mut group = c.benchmark_group("config");
    group.bench_function("load", |b| b.iter(|| black_box(config.load())));
    group.bench_function("manifest", |b| b.iter(|| black_box(config.manifest())));
    group.finish();
}

fn bench_mf2(c: &mut Criterion) {
    c.bench_function("mf2/format", |b| b.iter(|| black_box(mf2::format())));
}

criterion_group!(
    benches,
    bench_render,
    bench_action,
    bench_query,
    bench_migration,
    bench_sync,
    bench_config,
    bench_mf2
);
criterion_main!(benches);
