use fundamental_core::AnyError;
use leptos::prelude::*;
use std::sync::Arc;

type ErrorRenderer = dyn Fn(AnyError, Option<AnyView>, Callback<()>) -> AnyView + Send + Sync;

/// Renders an async error, optional stale content, and a retry callback.
#[derive(Clone)]
pub struct AsyncErrorView(Arc<ErrorRenderer>);

impl AsyncErrorView {
    /// Creates an error renderer with inferred callback argument types.
    pub fn new<F, V>(render: F) -> Self
    where
        F: Fn(AnyError, Option<AnyView>, Callback<()>) -> V + Send + Sync + 'static,
        V: IntoView + 'static,
    {
        Self(Arc::new(move |error, stale, retry| {
            render(error, stale, retry).into_any()
        }))
    }

    pub(super) fn render(
        &self,
        error: AnyError,
        stale: Option<AnyView>,
        retry: Callback<()>,
    ) -> AnyView {
        (self.0)(error, stale, retry)
    }
}
