//! [`AnyRelation`]: authorized at whichever relation the guard checked.

/// The relation of an [`Authorized`](super::Authorized) that does not say which was granted.
///
/// The default. It implements no [`RelationOf`](crate::schema::RelationOf), so it never
/// satisfies a page loader asking for `Authorized<N, R>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AnyRelation;
