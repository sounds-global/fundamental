use super::Database;

/// A type's bound database as an associated type; `#[derive(Model)]` emits it beside [`BoundTo<D>`](super::BoundTo).
pub trait HasDatabaseAffinity {
    /// The database type this model is bound to.
    type Database: Database;
}
