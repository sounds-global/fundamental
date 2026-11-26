use super::{AsyncErrorView, AsyncSuccessView, Writes};
use fundamental_core::AnyError;
use fundamental_core::action::Outcome;
use leptos::prelude::*;
use std::sync::Arc;

type SimpleView = Arc<dyn Fn() -> AnyView + Send + Sync>;
type ErrorView = Arc<dyn Fn(AnyError, Option<AnyView>, Callback<()>) -> AnyView + Send + Sync>;

/// Shared rendering defaults that can be installed around pages and hydrated islands.
#[derive(Clone)]
pub struct AsyncDefaultsConfig {
    pub(super) query_pending: SimpleView,
    pub(super) query_refreshing: SimpleView,
    pub(super) query_error: ErrorView,
    pub(super) mutation_pending: SimpleView,
    pub(super) mutation_success: AsyncSuccessView,
    pub(super) mutation_error: ErrorView,
}

fn view_fn(view: impl Into<ViewFn>) -> SimpleView {
    let view = view.into();
    Arc::new(move || view.run())
}

fn default_error(error: AnyError, stale: Option<AnyView>, _retry: Callback<()>) -> AnyView {
    view! {
        {stale}
        <p>{error.to_string()}</p>
    }
    .into_any()
}

fn default_success(outcome: Outcome<()>) -> AnyView {
    outcome
        .said
        .map(|said| view! { <p role="status">{said}</p> }.into_any())
        .unwrap_or_else(|| ().into_view().into_any())
}

impl Default for AsyncDefaultsConfig {
    fn default() -> Self {
        Self {
            query_pending: view_fn(|| view! { "Loading…" }),
            query_refreshing: view_fn(|| view! { "Refreshing…" }),
            query_error: Arc::new(default_error),
            mutation_pending: view_fn(|| view! { "Saving…" }),
            mutation_success: AsyncSuccessView::new(default_success),
            mutation_error: Arc::new(default_error),
        }
    }
}

impl AsyncDefaultsConfig {
    /// Sets the view shown while a query loads for the first time.
    pub fn query_pending(mut self, view: impl Into<ViewFn>) -> Self {
        self.query_pending = view_fn(view);
        self
    }

    /// Sets the view shown beside stale data during a query refresh.
    pub fn query_refreshing(mut self, view: impl Into<ViewFn>) -> Self {
        self.query_refreshing = view_fn(view);
        self
    }

    /// Sets the renderer for query errors, stale data, and retry.
    pub fn query_error(mut self, view: AsyncErrorView) -> Self {
        self.query_error = Arc::new(move |error, stale, retry| view.render(error, stale, retry));
        self
    }

    /// Sets the view shown while a mutation is in flight.
    pub fn mutation_pending(mut self, view: impl Into<ViewFn>) -> Self {
        self.mutation_pending = view_fn(view);
        self
    }

    /// Sets the view shown after a mutation succeeds, drawn from the write's outcome.
    pub fn mutation_success(mut self, view: impl Into<AsyncSuccessView>) -> Self {
        self.mutation_success = view.into();
        self
    }

    /// Sets the renderer for mutation errors and retry.
    pub fn mutation_error(mut self, view: AsyncErrorView) -> Self {
        self.mutation_error = Arc::new(move |error, stale, retry| view.render(error, stale, retry));
        self
    }

    /// Installs these defaults in the current Leptos owner.
    pub fn provide(&self) {
        provide_context(self.clone());
        Writes::provide();
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn the_success_view_is_drawn_from_the_outcome() {
        let owner = Owner::new();
        owner.with(|| {
            let config = AsyncDefaultsConfig::default()
                .mutation_success(|outcome: Outcome<()>| view! { <b>{outcome.said}</b> });
            let html = config
                .mutation_success
                .render(Outcome::nothing().said("Saved."))
                .to_html();
            assert!(html.contains("<b>Saved.</b>"), "{html}");
        });
    }

    #[test]
    fn the_default_success_view_says_the_outcomes_words() {
        let owner = Owner::new();
        owner.with(|| {
            let html = AsyncDefaultsConfig::default()
                .mutation_success
                .render(Outcome::nothing().said("Saved."))
                .to_html();
            assert!(html.contains(r#"role="status""#), "{html}");
            assert!(html.contains("Saved."), "{html}");
        });
    }

    #[test]
    fn the_default_success_view_draws_nothing_without_words() {
        let owner = Owner::new();
        owner.with(|| {
            let html = AsyncDefaultsConfig::default()
                .mutation_success
                .render(Outcome::nothing())
                .to_html();
            assert!(!html.contains("<p"), "{html}");
        });
    }
}
