use super::defaults::current_defaults;
use super::query_source::QuerySource;
use crate::QueryState;
use fundamental_core::AnyError;
use leptos::prelude::*;

/// Renders a query resource's pending, refreshing, failed, or successful state.
#[component]
pub fn Query<S, C, V>(resource: S, children: C) -> impl IntoView
where
    S: QuerySource,
    S::Resource: Clone + Send + Sync + IntoFuture<Output = Result<S::Value, AnyError>> + 'static,
    <S::Resource as IntoFuture>::IntoFuture: Send + 'static,
    C: Fn(S::Value) -> V + Send + Sync + Clone + 'static,
    V: IntoView + 'static,
{
    let query = resource.into_resource();
    let state = query.state();
    let retry = query.retry_callback();
    let defaults = current_defaults();
    let pending = defaults.query_pending.clone();
    let fallback_state = state;
    let fallback_children = children.clone();
    let fallback_refreshing = defaults.query_refreshing.clone();
    let resource = (*query).clone();
    view! {
        <Suspense fallback=move || {
            let state = fallback_state;
            let children = fallback_children.clone();
            let pending = pending.clone();
            let refreshing = fallback_refreshing.clone();
            move || match state.get() {
                QueryState::Refreshing(value) => view! {
                    {children(value)}
                    {refreshing()}
                }.into_any(),
                QueryState::Ready(value) => children(value).into_any(),
                QueryState::Failed { stale: Some(value), .. } => children(value).into_any(),
                QueryState::Pending | QueryState::Failed { stale: None, .. } => pending(),
            }
        }>
            {move || {
                let resource = resource.clone();
                let defaults = defaults.clone();
                let children = children.clone();
                Suspend::new(async move {
                    let result = resource.await;
                    state.set(QueryState::from_result(result));
                    move || match state.get() {
                        QueryState::Pending => (defaults.query_pending)(),
                        QueryState::Ready(value) => children(value).into_any(),
                        QueryState::Refreshing(value) => view! {
                            {children(value)}
                            {(defaults.query_refreshing)()}
                        }
                        .into_any(),
                        QueryState::Failed { error, stale } => {
                            let stale_view = stale.map(|value| children(value).into_any());
                            (defaults.query_error)(error, stale_view, retry)
                        }
                    }
                })
            }}
        </Suspense>
    }
}
