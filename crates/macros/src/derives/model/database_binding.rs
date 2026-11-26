/// The online backend a model is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DatabaseBinding {
    /// YSQL.
    SqlDb,
    /// YCQL.
    CqlDb,
}
