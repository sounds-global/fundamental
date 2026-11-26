use crate::Sealed;

use core::fmt;

/// Marker trait for database types; sealed.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid database marker",
    label = "not a database type",
    note = "Valid database markers are `SqlDb` and `CqlDb`."
)]
pub trait Database: Sealed + fmt::Debug + Send + Sync + 'static {
    /// The name of this database type.
    const NAME: &'static str;

    /// Whether this database supports transactions.
    const SUPPORTS_TRANSACTIONS: bool;

    /// Whether this database supports joins.
    const SUPPORTS_JOINS: bool;

    /// Whether this database supports secondary indexes.
    const SUPPORTS_SECONDARY_INDEXES: bool;
}
