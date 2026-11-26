//! Query role marker for reactive actions.

use super::operation::{Operation, ReactiveOperation};

/// A reactive action whose result is loaded as a Leptos query resource.
pub struct QueryOperation;

impl Operation for QueryOperation {}
impl ReactiveOperation for QueryOperation {}
