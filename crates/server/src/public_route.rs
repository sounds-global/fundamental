/// A route this surface serves to a caller with no principal, matched on the route template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicRoute {
    /// Exactly this template.
    Exact(&'static str),
    /// This template and everything under it — a documentation tree, a legal section.
    Tree(&'static str),
}

impl PublicRoute {
    /// Whether `template` is served without a principal.
    #[must_use]
    pub fn matches(&self, template: &str) -> bool {
        match self {
            Self::Exact(exact) => template == *exact,
            // The boundary is checked, so `/legal` does not open `/legalese`.
            Self::Tree(prefix) => {
                template == *prefix
                    || template
                        .strip_prefix(*prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
            }
        }
    }

    /// Exactly these templates, and nothing beneath them.
    #[must_use]
    pub fn exactly(templates: impl IntoIterator<Item = &'static str>) -> Vec<Self> {
        templates.into_iter().map(Self::Exact).collect()
    }

    /// The template this entry names, for diagnostics.
    #[must_use]
    pub const fn template(&self) -> &'static str {
        match self {
            Self::Exact(template) | Self::Tree(template) => template,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PublicRoute;

    #[test]
    fn exactly_names_every_template_it_was_given_and_no_others() {
        let public = PublicRoute::exactly(["/about", "/login"]);
        assert_eq!(public.len(), 2);
        assert!(public.iter().any(|route| route.matches("/about")));
        assert!(public.iter().any(|route| route.matches("/login")));
        assert!(!public.iter().any(|route| route.matches("/")));
    }

    #[test]
    fn exactly_opens_nothing_beneath_the_templates_it_names() {
        let public = PublicRoute::exactly(["/login"]);
        assert!(!public.iter().any(|route| route.matches("/login/passkey")));
    }

    #[test]
    fn exactly_of_nothing_admits_nothing() {
        assert!(PublicRoute::exactly([]).is_empty());
    }
}
