/// A hand-written documentation page shown in the dev-tools beside the generated reference.
///
/// The body is trusted HTML, rendered verbatim.
///
/// ```
/// # #[cfg(feature = "ssr")] {
/// use fundamental_introspect::DocPage;
///
/// let page = DocPage::new("Overview", "<p>What this service does.</p>")
///     .group("Guides")
///     .order(0);
/// assert_eq!(page.title(), "Overview");
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DocPage {
    title: String,
    group: String,
    order: i32,
    body: String,
}

impl DocPage {
    const DEFAULT_GROUP: &'static str = "Docs";

    /// Builds a page with a `title` and a `body` of trusted HTML, in the default group.
    #[must_use]
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            group: Self::DEFAULT_GROUP.to_string(),
            order: 0,
            body: body.into(),
        }
    }

    /// Sets the sidebar group this page files under (e.g. `"Guides"`).
    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    /// Sets the page's sort order within its group (lower renders first).
    #[must_use]
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// The page's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn group_name(&self) -> &str {
        &self.group
    }

    pub(crate) fn sort_order(&self) -> i32 {
        self.order
    }

    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_group_and_order() {
        let page = DocPage::new("Overview", "<p>Hi</p>");
        assert_eq!(page.group_name(), "Docs");
        assert_eq!(page.sort_order(), 0);
        assert_eq!(page.body(), "<p>Hi</p>");
    }

    #[test]
    fn builder_setters_override_group_and_order() {
        let page = DocPage::new("Auth", "<p>x</p>").group("Guides").order(3);
        assert_eq!(page.group_name(), "Guides");
        assert_eq!(page.sort_order(), 3);
    }
}
