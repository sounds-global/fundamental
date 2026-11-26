use fundamental_core::action::{Action, ActionPath};

use super::HookPath;

/// The default path state of an action hook: the action's own declared path.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DeclaredPath;

impl<M: Action> HookPath<M> for DeclaredPath {
    fn current(&self) -> ActionPath<M> {
        ActionPath::filled(M::PATH.to_owned())
    }
}
