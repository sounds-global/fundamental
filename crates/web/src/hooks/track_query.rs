use super::QueryState;
use fundamental_core::AnyError;
use leptos::prelude::*;
use std::future::Future;

/// Marks a request pending or refreshing, then stores its eventual result.
#[doc(hidden)]
pub async fn track_query<T>(
    state: RwSignal<QueryState<T>>,
    future: impl Future<Output = Result<T, AnyError>>,
) -> Result<T, AnyError>
where
    T: Clone + Send + Sync + 'static,
{
    let stale = state.with_untracked(QueryState::stale_value);
    state.set(match stale.clone() {
        Some(value) => QueryState::Refreshing(value),
        None => QueryState::Pending,
    });

    match future.await {
        Ok(value) => {
            state.set(QueryState::Ready(value.clone()));
            Ok(value)
        }
        Err(error) => {
            state.set(QueryState::Failed {
                error: error.clone(),
                stale,
            });
            Err(error)
        }
    }
}
