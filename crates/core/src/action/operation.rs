//! Type-level operation roles for reactive client actions.

/// Marks the semantic role of a client action independently from its transport method.
pub trait Operation: 'static {}

/// An action operation that can back a reactive query or mutation hook.
pub trait ReactiveOperation: Operation {}
