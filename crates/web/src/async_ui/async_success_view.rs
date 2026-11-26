use fundamental_core::action::Outcome;
use leptos::prelude::*;
use std::sync::Arc;

type SuccessRenderer = dyn Fn(Outcome<()>) -> AnyView + Send + Sync;

/// Renders a write's [`Outcome`]: the toast with its words, and its change id for Undo.
#[derive(Clone)]
pub struct AsyncSuccessView(Arc<SuccessRenderer>);

impl AsyncSuccessView {
    /// Creates a success renderer from a closure over the write's outcome.
    pub fn new<F, V>(render: F) -> Self
    where
        F: Fn(Outcome<()>) -> V + Send + Sync + 'static,
        V: IntoView + 'static,
    {
        Self(Arc::new(move |outcome| render(outcome).into_any()))
    }

    pub(super) fn render(&self, outcome: Outcome<()>) -> AnyView {
        (self.0)(outcome)
    }
}

impl<F, V> From<F> for AsyncSuccessView
where
    F: Fn(Outcome<()>) -> V + Send + Sync + 'static,
    V: IntoView + 'static,
{
    fn from(render: F) -> Self {
        Self::new(render)
    }
}
