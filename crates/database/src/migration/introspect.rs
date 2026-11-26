//! Reading a live database's schema back into a [`DatabaseSchema`]; each backend implements it.

use std::future::Future;

use crate::DatabaseResult;
use crate::migration::DatabaseSchema;

/// Reads the live schema of a database into a [`DatabaseSchema`].
///
/// Implemented per backend on a zero-sized introspector type, such as `OfflineIntrospector`.
pub trait SchemaIntrospector {
    /// The live connection type this introspector reads from.
    type Conn: ?Sized;

    /// Reads the current schema of `conn`.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`](crate::DatabaseError) if the connection or an introspection
    /// query fails, or a row cannot be read as a column definition.
    fn introspect(conn: &Self::Conn) -> impl Future<Output = DatabaseResult<DatabaseSchema>>;
}
