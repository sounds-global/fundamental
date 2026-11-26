//! The [`LeadingKeyAddressed`] constraint marker.

use core::marker::PhantomData;

use super::{
    BoundTo, CqlDb, Database, HasLeadingKey, NoLeadingKey, Offline, Satisfied, SingleKeyModel,
    SqlDb,
};

/// Marker asserting a query addresses rows through a usable key prefix.
///
/// Without it a query over a `(zone, …)` key that names no zone scans every zone. A new [`Database`] gets no
/// [`NoLeadingKey`] impl, so its unpinned reads fail to compile until one is stated.
#[derive(Debug)]
pub struct LeadingKeyAddressed<M, D, State>(PhantomData<(M, D, State)>);

/// A pinned query addresses rows through its leading key column, whatever the engine.
impl<M, D> Satisfied for LeadingKeyAddressed<M, D, HasLeadingKey>
where
    M: BoundTo<D>,
    D: Database,
{
}

/// On YSQL an *unpinned* query is addressable only when the whole key is one column.
impl<M> Satisfied for LeadingKeyAddressed<M, SqlDb, NoLeadingKey> where M: SingleKeyModel {}

/// The offline store holds one device's own rows, so it has no tenancy prefix to omit.
impl<M> Satisfied for LeadingKeyAddressed<M, Offline, NoLeadingKey> where M: BoundTo<Offline> {}

/// YCQL refuses an unpinned read through its own query builder's type state, so this marker does not judge it.
impl<M> Satisfied for LeadingKeyAddressed<M, CqlDb, NoLeadingKey> where M: BoundTo<CqlDb> {}
