//! The [`Satisfied`] constraint-witness trait.

use super::{BoundTo, Database, SameDatabase};

/// Marker trait indicating a constraint is satisfied.
pub trait Satisfied {}

impl<A, B, D> Satisfied for SameDatabase<A, B, D>
where
    A: BoundTo<D>,
    B: BoundTo<D>,
    D: Database,
{
}
