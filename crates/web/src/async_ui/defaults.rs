use super::async_defaults_config::AsyncDefaultsConfig;
use super::async_error_view::AsyncErrorView;
use super::async_success_view::AsyncSuccessView;
use leptos::prelude::*;

pub(super) fn current_defaults() -> AsyncDefaultsConfig {
    use_context().unwrap_or_default()
}

/// Provides shared query and mutation feedback throughout a Leptos view tree.
#[component]
pub fn AsyncDefaults(
    children: Children,
    #[prop(optional)] config: Option<AsyncDefaultsConfig>,
    #[prop(optional, into)] query_pending: Option<ViewFn>,
    #[prop(optional, into)] query_refreshing: Option<ViewFn>,
    #[prop(optional)] query_error: Option<AsyncErrorView>,
    #[prop(optional, into)] mutation_pending: Option<ViewFn>,
    #[prop(optional, into)] mutation_success: Option<AsyncSuccessView>,
    #[prop(optional)] mutation_error: Option<AsyncErrorView>,
) -> impl IntoView {
    let mut defaults = config.unwrap_or_default();
    if let Some(view) = query_pending {
        defaults = defaults.query_pending(view);
    }
    if let Some(view) = query_refreshing {
        defaults = defaults.query_refreshing(view);
    }
    if let Some(view) = query_error {
        defaults = defaults.query_error(view);
    }
    if let Some(view) = mutation_pending {
        defaults = defaults.mutation_pending(view);
    }
    if let Some(view) = mutation_success {
        defaults = defaults.mutation_success(view);
    }
    if let Some(view) = mutation_error {
        defaults = defaults.mutation_error(view);
    }
    defaults.provide();
    children()
}
