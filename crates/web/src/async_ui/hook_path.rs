use fundamental_core::action::ActionPath;

/// The path an action hook calls its action at, read each time the hook fetches or submits.
#[doc(hidden)]
pub trait HookPath<M>: 'static {
    /// The path to call the action at now.
    fn current(&self) -> ActionPath<M>;
}
