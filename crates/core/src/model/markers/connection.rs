//! The [`Connection`] marker trait.

/// A logical database connection a model routes to: a name, bound to a pool per application at startup.
///
/// Declare one with `#[derive(Connection)]` and name it with `#[model(connection = …)]`; a model that names
/// none routes to [`Primary`](super::Primary). Unlike [`Database`](super::Database), it is not sealed.
///
/// ```rust
/// # extern crate fundamental_core as fundamental;
/// use fundamental::model::Connection;
///
/// # #[allow(dead_code)]
/// struct Reports;
/// impl Connection for Reports {
///     const NAME: &'static str = "reports";
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a database connection marker",
    label = "not a connection type",
    note = "Declare it with `#[derive(Connection)]` on a unit struct, e.g. `#[derive(Connection)] struct Reports;`."
)]
pub trait Connection {
    /// The routing name a connection registry resolves to a pool; the `snake_case` type name unless `#[connection(name = "…")]`.
    const NAME: &'static str;
}
