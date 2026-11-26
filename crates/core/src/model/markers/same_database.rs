//! The [`SameDatabase`] constraint marker.

/// Marker type for asserting two types are bound to the same database.
///
/// # Examples
///
/// ```no_run
/// use fundamental_core::model::{BoundTo, Database};
///
/// fn transfer<A, B, D>(from: &A, to: &B)
/// where
///     A: BoundTo<D>,
///     B: BoundTo<D>,
///     D: Database,
/// {
///     // Both A and B are guaranteed to be bound to the same database D
/// }
/// ```
#[derive(Debug)]
pub struct SameDatabase<A, B, D>(core::marker::PhantomData<(A, B, D)>);
