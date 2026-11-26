//! Integration tests that run in parallel on one shared database, each reaching only its own rows.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod function_scan;
mod isolation_error;
mod isolation_finding;
mod isolation_rule;
mod run_alone;
mod schema_change;
mod test_isolation;

pub use isolation_error::IsolationError;
pub use isolation_finding::IsolationFinding;
pub use isolation_rule::IsolationRule;
pub use run_alone::RunAlone;
pub use test_isolation::TestIsolation;
