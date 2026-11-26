//! [`Action`] — the portable contract describing a server action's wire shape.

/// The portable description of an HTTP action: the types it carries and the method
/// and path it is served at, on every build including wasm.
pub trait Action {
    /// The action's request body type — [`NoInput`](super::NoInput) when it takes none.
    type Input;
    /// The action's response type.
    type Output;
    /// The uppercase HTTP method token the action is served with (e.g. `"POST"`).
    const METHOD: &'static str;
    /// The path the action's route is registered at (e.g. `"/api/greet"`).
    const PATH: &'static str;
}
