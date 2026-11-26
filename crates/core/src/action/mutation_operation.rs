//! Mutation role marker for reactive actions.

use super::operation::{Operation, ReactiveOperation};

/// A reactive action dispatched as a Leptos mutation.
pub struct MutationOperation;

impl Operation for MutationOperation {}
impl ReactiveOperation for MutationOperation {}
