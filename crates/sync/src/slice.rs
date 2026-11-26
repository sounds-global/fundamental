//! The [`Slice`] trait: a slice's portable, surface-agnostic metadata.

use fundamental_core::validate::Validate;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ServerSyncable;

/// The portable identity of a slice: a named, principal-scoped, parameterized query over a
/// syncable base table.
pub trait Slice {
    /// The syncable base table the slice draws from.
    type Model: ServerSyncable;

    /// The client-supplied parameters; [`NoParams`](crate::NoParams) for a slice scoped by the
    /// principal alone.
    type Params: Serialize + DeserializeOwned + Validate + Send;

    /// The slice's stable name in the registry and on the wire.
    const NAME: &'static str;
}
