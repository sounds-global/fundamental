/// Compile-time metadata describing an HTTP action for API tooling, implemented by `#[action]`.
pub trait ActionMeta {
    /// The action's `///` documentation, joined across lines (empty when undocumented).
    const DOC: &'static str;
    /// The names of the middleware in the action's chain, in declaration order.
    const MIDDLEWARE: &'static [&'static str];

    /// Why this action's act names no single object, or `None` when it names one.
    ///
    /// Set by `#[action(…, no_single_object = "…")]`, so every exemption can be enumerated.
    const NO_SINGLE_OBJECT: Option<&'static str> = None;
}
