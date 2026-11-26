use super::defaults::current_defaults;
use super::mutation_scope::MutationScope;
use super::mutation_source::MutationSource;
use super::writes::Writes;
use leptos::prelude::*;

/// Provides mutation dispatch and configured feedback to its children.
#[component]
pub fn Mutation<S, C, V>(mutation: S, children: C) -> impl IntoView
where
    S: MutationSource,
    C: Fn(MutationScope<S::Input, S::Changed>) -> V + Send + Sync + Clone + 'static,
    V: IntoView + 'static,
{
    let mutation = mutation.into_action();
    let last_input = RwSignal::new(None::<S::Input>);
    let dispatch_action = mutation;
    let submit_callback = Callback::new(move |input: S::Input| {
        last_input.set(Some(input.clone()));
        dispatch_action.dispatch(input);
    });
    let scope = MutationScope {
        submit_callback,
        pending_signal: mutation.pending(),
        value_signal: {
            let value = mutation.value();
            Signal::derive(move || value.get())
        },
    };
    // Bumping the page's write count makes every query read again; effects run only in the browser.
    if let Some(writes) = Writes::current() {
        let version = mutation.version();
        let answer = mutation.value();
        Effect::new(move |_| {
            version.track();
            answer.with_untracked(|answer| writes.record_if_written(answer.as_ref()));
        });
    }
    let defaults = current_defaults();
    let content = children(scope.clone());
    let pending = scope.pending_signal;
    let value = scope.value_signal;
    view! {
        {content}
        {move || {
            if pending.get() {
                (defaults.mutation_pending)()
            } else {
                match value.get() {
                    Some(Ok(outcome)) => defaults.mutation_success.render(outcome.without_value()),
                    Some(Err(error)) => {
                        let retry_action = mutation;
                        let retry_input = last_input;
                        let retry = Callback::new(move |()| {
                            if let Some(input) = retry_input.get_untracked() {
                                retry_action.dispatch(input);
                            }
                        });
                        (defaults.mutation_error)(error, None, retry)
                    }
                    None => ().into_view().into_any(),
                }
            }
        }}
    }
}
