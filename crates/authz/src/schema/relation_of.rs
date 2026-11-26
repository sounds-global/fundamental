//! [`RelationOf`]: one relation of a namespace, as a type.

use super::{Namespace, Relation};

/// A relation of the namespace `N`, named by a type rather than a value.
///
/// `#[derive(Namespace)]` emits one marker type per relation (`team_relation::Member`), so a
/// signature such as `Authorized<Team, team_relation::Admin>` states its relation.
pub trait RelationOf<N>: Send + Sync + 'static
where
    N: Namespace + Relation,
{
    /// The relation this type names.
    const RELATION: N;

    /// The relation's name, as the schema and a recorded grant spell it.
    #[must_use]
    fn relation_name() -> &'static str {
        Self::RELATION.relation_name()
    }
}
