use super::sealed::Sealed;

/// Whether a policy guards an object: [`Open`](crate::Open) or [`Guarded`](crate::Guarded).
///
/// A type, so a guarded chain over a state with no authorizer is a compile error.
pub trait GuardState: Sealed + Send + Sync + 'static {}
