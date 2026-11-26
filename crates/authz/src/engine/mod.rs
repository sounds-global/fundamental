//! The check evaluation engine: the bounded, caveat-aware walk of the rewrite algebra.

mod caveat_eval;
mod check_path;
// `Engine` lives in `engine.rs`, one object per file.
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod engine;
mod eval_context;
mod eval_limits;
mod justification;
mod refuse_request_context;
mod set_index;
mod transitive_closure_index;
mod userset_tree;
mod witness;

pub use engine::Engine;
pub use eval_limits::EvalLimits;
pub(crate) use justification::Justification;
pub(crate) use refuse_request_context::refuse_request_context;
pub use set_index::SetIndex;
pub use transitive_closure_index::TransitiveClosureIndex;
pub use userset_tree::UsersetTree;
pub(crate) use witness::Witness;
