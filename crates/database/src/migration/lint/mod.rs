//! Migration lints: what a migration can do that is safe to write and dangerous to run, held
//! to the level the project sets in `fun.toml` (see [`Lint`] and [`Lints`]).
//!
//! `refuse` blocks, `warn` blocks until the table's model accepts the lint with a reason, and
//! `allow` turns it off. "A table with rows" is one its file did not create.

mod facts;
pub(crate) mod judge;
mod lint_finding;
mod lints;
mod lints_error;
pub(crate) mod verdict;

pub use fundamental_core::model::sql::{Level, Lint};
pub use fundamental_macros::lints;
pub(crate) use judge::narrows;
pub use lint_finding::LintFinding;
pub(crate) use lint_finding::report;
pub use lints::Lints;
pub use lints_error::LintsError;
