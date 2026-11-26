use super::QueryState;
use leptos::prelude::*;
use std::ops::Deref;

/// A Leptos resource together with Fundamental's query lifecycle state; derefs to the resource.
pub struct QueryResource<R, T> {
    resource: R,
    state: RwSignal<QueryState<T>>,
    retry: Callback<(), ()>,
}

impl<R, T> QueryResource<R, T> {
    /// Builds a query handle around a Leptos resource and its tracked state.
    #[doc(hidden)]
    pub fn new(resource: R, state: RwSignal<QueryState<T>>, retry: Callback<(), ()>) -> Self {
        Self {
            resource,
            state,
            retry,
        }
    }

    /// Returns the reactive query lifecycle state.
    pub fn state(&self) -> RwSignal<QueryState<T>> {
        self.state
    }

    /// Retries the resource's most recent source.
    pub fn retry(&self) {
        self.retry.run(());
    }

    /// Returns the retry callback for use in a view.
    pub fn retry_callback(&self) -> Callback<(), ()> {
        self.retry
    }
}

impl<R, T> Deref for QueryResource<R, T> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        &self.resource
    }
}

impl<R: Copy, T> Copy for QueryResource<R, T> {}

impl<R: Copy, T> Clone for QueryResource<R, T> {
    fn clone(&self) -> Self {
        *self
    }
}
