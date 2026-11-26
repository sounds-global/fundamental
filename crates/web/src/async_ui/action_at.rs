use fundamental_core::action::ActionPath;

use super::HookPath;

/// The closure `.at(..)` gave an action hook, filling the action's path from state.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct At<F>(pub(super) F);

impl<M, F> HookPath<M> for At<F>
where
    F: Fn() -> ActionPath<M> + 'static,
{
    fn current(&self) -> ActionPath<M> {
        (self.0)()
    }
}
