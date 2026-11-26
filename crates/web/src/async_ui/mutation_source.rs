use super::{ActionHook, HookPath, LocalSource, NoQuerySource};
use fundamental_core::{
    AnyError,
    action::{Action as ActionMeta, ClientAction, MutationOperation, Outcome},
};
use leptos::prelude::Action;

/// Converts a Leptos Action or generated action hook into the action rendered by [`Mutation`](super::Mutation).
#[doc(hidden)]
pub trait MutationSource: 'static {
    /// The action's dispatch input (`()` for no-input actions).
    type Input: Clone + Send + Sync + 'static;
    /// What the write changed: the `T` of the [`Outcome<T>`] it answers.
    type Changed: Clone + Send + Sync + 'static;

    /// Builds the Leptos Action inside the component's reactive owner.
    fn into_action(self) -> Action<Self::Input, Result<Outcome<Self::Changed>, AnyError>>;
}

impl<I, T> MutationSource for Action<I, Result<Outcome<T>, AnyError>>
where
    I: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    type Input = I;
    type Changed = T;

    fn into_action(self) -> Action<Self::Input, Result<Outcome<Self::Changed>, AnyError>> {
        self
    }
}

impl<M, I, T, P> MutationSource for ActionHook<M, MutationOperation, NoQuerySource, P>
where
    M: ActionMeta<Input = I>
        + ClientAction<Success = Outcome<T>, Operation = MutationOperation>
        + 'static,
    I: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    P: HookPath<M> + Send + Sync,
{
    type Input = I;
    type Changed = T;

    fn into_action(self) -> Action<Self::Input, Result<Outcome<Self::Changed>, AnyError>> {
        let path = self.path;
        crate::use_mutation(move |input: I| {
            <M as ClientAction>::call_at::<crate::WebActionTransport>(path.current(), input)
        })
    }
}

impl<M, I, T, P> MutationSource for ActionHook<M, MutationOperation, LocalSource<NoQuerySource>, P>
where
    M: ActionMeta<Input = I>
        + ClientAction<Success = Outcome<T>, Operation = MutationOperation>
        + 'static,
    I: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    P: HookPath<M>,
{
    type Input = I;
    type Changed = T;

    fn into_action(self) -> Action<Self::Input, Result<Outcome<Self::Changed>, AnyError>> {
        let path = self.path;
        crate::use_mutation_local(move |input: I| {
            <M as ClientAction>::call_at::<crate::WebActionTransport>(path.current(), input)
        })
    }
}
