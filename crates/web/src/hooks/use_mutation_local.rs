use super::MutationFunction;
use leptos::prelude::*;

/// Creates a local Leptos Action from a Fundamental browser action; inputs need not be `Send`.
pub fn use_mutation_local<F, Args>(action_fn: F) -> Action<F::Input, F::Output>
where
    F: MutationFunction<Args> + 'static,
    F::Input: Clone + 'static,
    F::Output: 'static,
{
    Action::new_unsync_local(move |input: &F::Input| action_fn.invoke(input.clone()))
}
