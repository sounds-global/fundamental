use crate::in_language::in_language;
use crate::page_context::PageContext;

/// What [`render_page`](crate::render_page) produces: the page body and the [`PageContext`] it saw.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    context: PageContext,
    body: String,
}

impl RenderedPage {
    /// Pairs a rendered `body` with the `context` that produced it.
    #[must_use]
    pub fn new(context: PageContext, body: String) -> Self {
        Self { context, body }
    }

    /// The context the page rendered with — its reader, its language, its params.
    #[must_use]
    pub fn context(&self) -> &PageContext {
        &self.context
    }

    /// The rendered page body, to be wrapped in a document shell.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Wraps the body in `shell`, inside the language the page rendered in.
    #[must_use]
    pub fn into_document(self, shell: impl FnOnce(&PageContext, &str) -> String) -> String {
        in_language(self.context.locale(), || shell(&self.context, &self.body))
    }

    /// Consumes this and returns the body alone.
    #[must_use]
    pub fn into_body(self) -> String {
        self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rendered_page_carries_both_halves() {
        let context = PageContext::new(Vec::new(), "req-1");
        let rendered = RenderedPage::new(context, "<p>hi</p>".to_owned());
        assert_eq!(rendered.context().request_id(), "req-1");
        assert_eq!(rendered.body(), "<p>hi</p>");
        assert_eq!(rendered.into_body(), "<p>hi</p>");
    }
}
