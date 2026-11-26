//! The app the load, soak and fault runs drive, served in process over YugabyteDB and NATS.
//!
//! Built only with debug assertions (the `bench-live` profile): the containers' fixtures and
//! plain-text development connections exist only with them.

mod app;
mod models;

pub use app::{BenchApp, Engines};

use std::path::PathBuf;

/// Where a run writes its JSON: `BENCH_OUT`, or `target/bench` in the workspace.
pub fn out_dir() -> PathBuf {
    std::env::var_os("BENCH_OUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bench"),
        PathBuf::from,
    )
}
