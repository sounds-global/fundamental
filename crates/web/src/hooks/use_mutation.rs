use super::MutationFunction;
use leptos::prelude::*;

/// Creates a Leptos Action for a Fundamental action with thread-safe input and output.
#[cfg(not(target_arch = "wasm32"))]
pub fn use_mutation<F, Args>(action_fn: F) -> Action<F::Input, F::Output>
where
    F: MutationFunction<Args> + Send + Sync + 'static,
    F::Input: Clone + Send + Sync + 'static,
    F::Output: Send + Sync + 'static,
    F::Fut: Send,
{
    Action::new(move |input: &F::Input| action_fn.invoke(input.clone()))
}

/// Creates a Leptos Action on the local executor: browser transport futures are not `Send`.
#[cfg(target_arch = "wasm32")]
pub fn use_mutation<F, Args>(action_fn: F) -> Action<F::Input, F::Output>
where
    F: MutationFunction<Args> + Send + Sync + 'static,
    F::Input: Clone + Send + Sync + 'static,
    F::Output: Send + Sync + 'static,
{
    Action::new_local(move |input: &F::Input| action_fn.invoke(input.clone()))
}
