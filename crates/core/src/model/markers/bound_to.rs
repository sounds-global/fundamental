use super::Database;

/// Marker trait for a model bound to database `D`, emitted by `#[derive(Model)]` from its backend attribute.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not bound to this database",
    label = "this model cannot be used with this database",
    note = "Use `#[derive(Model)]` with `#[model(database = \"...\")]` to bind to a database."
)]
pub trait BoundTo<D: Database> {}
