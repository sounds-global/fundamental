//! Storage backends for the relation-tuple store, each behind its cargo feature (`sql`,
//! `cql`, `offline`).

#[cfg(any(
    feature = "sql",
    feature = "cql",
    all(feature = "offline", not(target_arch = "wasm32"))
))]
pub(crate) mod columns;
#[cfg(any(
    feature = "sql",
    feature = "cql",
    all(feature = "offline", not(target_arch = "wasm32"))
))]
pub(crate) mod conversion;

#[cfg(feature = "cql")]
mod cql;
#[cfg(all(feature = "offline", not(target_arch = "wasm32")))]
mod offline;
#[cfg(feature = "sql")]
mod sql;

#[cfg(feature = "cql")]
pub use cql::CqlTupleStore;
#[cfg(all(feature = "offline", not(target_arch = "wasm32")))]
pub use offline::OfflineTupleStore;
// Crate-private: the view exists only inside a `TransactionalAuthorizer` decision.
#[cfg(feature = "sql")]
pub(crate) use sql::SqlTransactionView;
// `SqlReadScope` is `SqlTupleStore`'s `TupleStore::Scope`, so callers must be able to name it.
#[cfg(feature = "sql")]
pub use sql::{SQL_TUPLE_MIGRATIONS, SqlReadScope, SqlTupleStore};
