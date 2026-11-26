use crate::page::PageTitle;

/// The parts of a render that differ between a legacy page and a page under the pipeline.
#[derive(Clone, Default)]
pub(crate) struct RenderOptions {
    /// Whether the application state is provided into the view's context.
    pub(crate) without_state: bool,
    /// The title a policy declared, set before the view so the page may still set its own.
    pub(crate) title: Option<PageTitle>,
}
