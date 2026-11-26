//! Benchmarks and stability runs for the framework: instruction counts checked against a
//! committed [`Baseline`], wall-clock benches, and load, soak and fault runs.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod baseline;
mod bench_error;
mod checked;
mod fault;
mod fault_report;
mod finding;
mod latencies;
#[cfg(all(feature = "live", debug_assertions))]
pub mod live;
mod live_args;
mod load;
mod measured;
mod report;
mod rss;
mod soak;
mod soak_report;
mod step;
#[cfg(feature = "workloads")]
pub mod workloads;

pub use baseline::{Baseline, DEFAULT_THRESHOLD};
pub use bench_error::BenchError;
pub use checked::Checked;
pub use fault::Fault;
pub use fault_report::FaultReport;
pub use finding::Finding;
pub use live_args::LiveArgs;
pub use load::Load;
pub use measured::Measured;
pub use report::Report;
pub use rss::Rss;
pub use soak::Soak;
pub use soak_report::SoakReport;
pub use step::Step;
