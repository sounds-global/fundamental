//! `fun bench`: the workspace's benchmarks, one verb per layer.

mod bench_args;
mod bench_target;
mod container;
mod run;

pub(crate) use bench_args::BenchArgs;
pub(crate) use run::run;
