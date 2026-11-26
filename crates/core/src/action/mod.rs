//! The portable action contract and the marker types for its wire shapes.

#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod action;
mod action_path;
mod change_id;
mod client_action;
mod mutation_operation;
mod no_input;
mod no_output;
mod operation;
mod outcome;
mod query_operation;

pub use action::Action;
pub use action_path::ActionPath;
pub use change_id::ChangeId;
pub use client_action::{ActionTransport, ClientAction};
pub use mutation_operation::MutationOperation;
pub use no_input::NoInput;
pub use no_output::NoOutput;
pub use operation::{Operation, ReactiveOperation};
pub use outcome::Outcome;
pub use query_operation::QueryOperation;
