//! The hot paths' instruction counts under callgrind, checked against
//! `benches/baseline/<arch>.json`.
//!
//! Setup and one warm call run before counting starts, and the fixture is dropped after it
//! stops. Run with `fun bench` (needs valgrind and `gungraun-runner` 0.20.0 on PATH).

#![expect(
    missing_docs,
    reason = "gungraun's macros generate undocumented modules, constants and functions"
)]

use std::hint::black_box;

use gungraun::prelude::*;

use fundamental_bench::workloads as w;
use fundamental_bench::workloads::action::ActionDispatch;
use fundamental_bench::workloads::config::ConfigLoad;
use fundamental_bench::workloads::migration::Migration;
use fundamental_bench::workloads::sync::SyncWire;

fn warm<T>(call: fn() -> T) {
    black_box(call());
}

fn warm_render() {
    warm(w::render::page);
}

#[library_benchmark]
#[bench::fifty_rows(setup = warm_render)]
fn page(_: ()) -> String {
    black_box(w::render::page())
}

library_benchmark_group!(name = render, benchmarks = [page]);

fn mounted() -> ActionDispatch {
    let dispatch = ActionDispatch::new();
    black_box(dispatch.dispatch());
    dispatch
}

#[library_benchmark]
#[bench::json_body(setup = mounted)]
fn dispatch(dispatch: ActionDispatch) -> (Option<u16>, ActionDispatch) {
    (black_box(dispatch.dispatch()), dispatch)
}

library_benchmark_group!(name = action, benchmarks = [dispatch]);

fn warm_query() {
    warm(w::query::select_sql);
    warm(w::query::select_cql);
}

#[library_benchmark]
#[bench::ten_filters(setup = warm_query)]
fn select_sql(_: ()) -> usize {
    black_box(w::query::select_sql())
}

#[library_benchmark]
#[bench::ten_columns(setup = warm_query)]
fn update_sql(_: ()) -> usize {
    black_box(w::query::update_sql())
}

#[library_benchmark]
#[bench::ten_clustering(setup = warm_query)]
fn select_cql(_: ()) -> usize {
    black_box(w::query::select_cql())
}

library_benchmark_group!(
    name = query,
    benchmarks = [select_sql, update_sql, select_cql]
);

fn schema() -> Migration {
    let migration = Migration::new();
    black_box(migration.generate());
    migration
}

#[library_benchmark]
#[bench::fifty_tables(setup = schema)]
fn diff(migration: Migration) -> (usize, Migration) {
    (black_box(migration.diff()), migration)
}

#[library_benchmark]
#[bench::fifty_tables(setup = schema)]
fn generate(migration: Migration) -> (Option<usize>, Migration) {
    (black_box(migration.generate()), migration)
}

#[library_benchmark]
#[bench::fifty_tables(setup = schema)]
fn differences(migration: Migration) -> (usize, Migration) {
    (black_box(migration.differences()), migration)
}

library_benchmark_group!(name = migration, benchmarks = [diff, generate, differences]);

fn wire() -> SyncWire {
    let wire = SyncWire::new();
    black_box(wire.decode_pull());
    wire
}

#[library_benchmark]
#[bench::hundred_rows(setup = wire)]
fn encode_pull(wire: SyncWire) -> (usize, SyncWire) {
    (black_box(wire.encode_pull()), wire)
}

#[library_benchmark]
#[bench::hundred_rows(setup = wire)]
fn decode_pull(wire: SyncWire) -> (usize, SyncWire) {
    (black_box(wire.decode_pull()), wire)
}

#[library_benchmark]
#[bench::one_row(setup = wire)]
fn push_round_trip(wire: SyncWire) -> (bool, SyncWire) {
    (black_box(wire.push_round_trip()), wire)
}

#[library_benchmark]
#[bench::one_row(setup = wire)]
fn canonical(wire: SyncWire) -> (usize, SyncWire) {
    (black_box(wire.canonical()), wire)
}

#[library_benchmark]
#[bench::one_frame(setup = wire)]
fn envelope_round_trip(wire: SyncWire) -> (bool, SyncWire) {
    (black_box(wire.envelope_round_trip()), wire)
}

library_benchmark_group!(
    name = sync,
    benchmarks = [
        encode_pull,
        decode_pull,
        push_round_trip,
        canonical,
        envelope_round_trip
    ]
);

#[expect(
    clippy::expect_used,
    reason = "a bench fixture; a failed write ends the run"
)]
fn project() -> ConfigLoad {
    let config = ConfigLoad::new().expect("a project directory");
    black_box(config.load());
    black_box(config.manifest());
    config
}

#[library_benchmark]
#[bench::production(setup = project)]
fn load(config: ConfigLoad) -> (Option<u16>, ConfigLoad) {
    (black_box(config.load()), config)
}

#[library_benchmark]
#[bench::production(setup = project)]
fn manifest(config: ConfigLoad) -> (Option<usize>, ConfigLoad) {
    (black_box(config.manifest()), config)
}

library_benchmark_group!(name = config, benchmarks = [load, manifest]);

fn warm_mf2() {
    warm(w::mf2::format);
}

#[library_benchmark]
#[bench::twelve_messages(setup = warm_mf2)]
fn format(_: ()) -> usize {
    black_box(w::mf2::format())
}

library_benchmark_group!(name = mf2, benchmarks = [format]);

main!(
    library_benchmark_groups = render,
    action,
    query,
    migration,
    sync,
    config,
    mf2
);
