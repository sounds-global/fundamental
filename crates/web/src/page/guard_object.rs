/// The object a page guard checks: a route parameter's value, or a fixed id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GuardObject {
    /// The route parameter of this name.
    Param(&'static str),
    /// This id, for every request.
    Const(String),
}
