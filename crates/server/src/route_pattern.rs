use crate::PathParams;

/// One segment of a route template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// A literal segment that must match exactly (for example `users`).
    Static(String),
    /// A capture, written `{name}` in the template (for example `{id}`).
    Param(String),
    /// A trailing catch-all, `{*name}`, capturing the remaining path, slashes included.
    CatchAll(String),
}

/// A route path template, parsed once at registration: `{name}` captures a segment, `{*name}` the rest.
#[derive(Debug, Clone)]
pub(crate) struct RoutePattern {
    segments: Vec<Segment>,
    /// Shared so a request can carry its own template without copying it.
    raw: std::sync::Arc<str>,
}

impl RoutePattern {
    /// Parses a route template; leading and trailing slashes are ignored.
    pub(crate) fn parse(template: &str) -> Self {
        let segments = split_segments(template)
            .map(|segment| match param_name(segment) {
                Some(name) if name.starts_with('*') => Segment::CatchAll(name[1..].to_owned()),
                Some(name) => Segment::Param(name.to_owned()),
                None => Segment::Static(segment.to_owned()),
            })
            .collect();
        Self {
            segments,
            raw: template.into(),
        }
    }

    /// Whether the template captures anything.
    pub(crate) fn captures_anything(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, Segment::Param(_) | Segment::CatchAll(_)))
    }

    /// Whether `segment` is one of the template's literal segments.
    pub(crate) fn has_literal(&self, segment: &str) -> bool {
        self.segments
            .iter()
            .any(|part| matches!(part, Segment::Static(literal) if literal == segment))
    }

    /// Returns the original template string.
    pub(crate) fn raw(&self) -> &str {
        &self.raw
    }

    /// The original template, shared.
    pub(crate) fn template(&self) -> std::sync::Arc<str> {
        std::sync::Arc::clone(&self.raw)
    }

    /// A sort key, most specific first: literal over parameter over catch-all, a prefix first.
    pub(crate) fn specificity(&self) -> Vec<u8> {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Static(_) => 0,
                Segment::Param(_) => 1,
                Segment::CatchAll(_) => 2,
            })
            .collect()
    }

    /// Whether `other` matches exactly the paths this template matches.
    pub(crate) fn same_shape(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(&other.segments)
                .all(|pair| match pair {
                    (Segment::Static(a), Segment::Static(b)) => a == b,
                    (Segment::Param(_), Segment::Param(_))
                    | (Segment::CatchAll(_), Segment::CatchAll(_)) => true,
                    _ => false,
                })
    }

    /// The template's literal segments.
    pub(crate) fn literals(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Static(literal) => Some(literal.as_str()),
            Segment::Param(_) | Segment::CatchAll(_) => None,
        })
    }

    /// The names of the template's captures, in path order.
    pub(crate) fn params(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|segment| match segment {
            Segment::Param(name) | Segment::CatchAll(name) => Some(name.as_str()),
            Segment::Static(_) => None,
        })
    }

    /// The path this template names with each capture spelled by `value_of`; `None` when one has no value.
    pub(crate) fn fill<'a>(&self, value_of: impl Fn(&str) -> Option<&'a str>) -> Option<String> {
        let mut path = String::new();
        for segment in &self.segments {
            path.push('/');
            match segment {
                Segment::Static(literal) => path.push_str(literal),
                Segment::Param(name) | Segment::CatchAll(name) => path.push_str(value_of(name)?),
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        Some(path)
    }

    /// Matches `path` against this template, capturing parameters on success.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn matches(&self, path: &str) -> Option<PathParams> {
        let mut path_segments = split_segments(path);
        let mut captured = Vec::new();

        for segment in &self.segments {
            match segment {
                Segment::CatchAll(name) => {
                    let rest = path_segments.by_ref().collect::<Vec<_>>().join("/");
                    captured.push((name.clone(), rest));
                    return Some(captured.into_iter().collect());
                }
                Segment::Static(literal) => {
                    if *literal != *path_segments.next()? {
                        return None;
                    }
                }
                Segment::Param(name) => {
                    captured.push((name.clone(), path_segments.next()?.to_owned()));
                }
            }
        }

        if path_segments.next().is_some() {
            return None;
        }

        Some(captured.into_iter().collect())
    }
}

/// Splits a path into its non-empty segments, ignoring leading/trailing slashes.
fn split_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|segment| !segment.is_empty())
}

/// Returns the capture name if `segment` is written `{name}`.
fn param_name(segment: &str) -> Option<&str> {
    segment
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_literal_outranks_a_parameter_and_a_parameter_a_catch_all() {
        let mut templates = [
            "/{team}/{*rest}",
            "/{team}/aanvragen",
            "/api/{id}",
            "/{team}",
            "/files",
            "/files/{*rest}",
        ]
        .map(RoutePattern::parse);
        templates.sort_by_key(RoutePattern::specificity);
        let order: Vec<&str> = templates.iter().map(RoutePattern::raw).collect();
        assert_eq!(
            order,
            [
                "/files",
                "/api/{id}",
                "/files/{*rest}",
                "/{team}",
                "/{team}/aanvragen",
                "/{team}/{*rest}"
            ]
        );
    }

    #[test]
    fn templates_of_one_shape_differ_only_in_their_names() {
        let a = RoutePattern::parse("/teams/{team}/x");
        assert!(a.same_shape(&RoutePattern::parse("/teams/{id}/x")));
        assert!(!a.same_shape(&RoutePattern::parse("/teams/{team}/y")));
        assert!(!a.same_shape(&RoutePattern::parse("/teams/{team}")));
    }

    #[test]
    fn a_template_fills_from_its_captures() {
        let pattern = RoutePattern::parse("/{team}/aanvragen/{build}");
        let filled = pattern.fill(|name| match name {
            "team" => Some("sounds-global"),
            "build" => Some("12"),
            _ => None,
        });
        assert_eq!(filled.as_deref(), Some("/sounds-global/aanvragen/12"));
        assert_eq!(pattern.fill(|_| None), None);
        assert_eq!(
            RoutePattern::parse("/").fill(|_| None).as_deref(),
            Some("/")
        );
        assert_eq!(pattern.params().collect::<Vec<_>>(), ["team", "build"]);
    }

    #[test]
    fn static_path_matches_exactly() {
        let pattern = RoutePattern::parse("/health");
        assert!(pattern.matches("/health").is_some());
        assert!(pattern.matches("/health/extra").is_none());
        assert!(pattern.matches("/other").is_none());
        assert_eq!(pattern.raw(), "/health");
    }

    #[test]
    fn captures_named_parameters() {
        let pattern = RoutePattern::parse("/users/{id}/posts/{slug}");
        let params = pattern
            .matches("/users/42/posts/hello")
            .expect("matches with captures");
        assert_eq!(params.get("id"), Some("42"));
        assert_eq!(params.get("slug"), Some("hello"));
    }

    #[test]
    fn segment_count_must_match() {
        let pattern = RoutePattern::parse("/users/{id}");
        assert!(pattern.matches("/users").is_none());
        assert!(pattern.matches("/users/1/extra").is_none());
        assert!(pattern.matches("/users/1").is_some());
    }

    #[test]
    fn trailing_and_leading_slashes_are_ignored() {
        let pattern = RoutePattern::parse("users/{id}/");
        assert!(pattern.matches("/users/9").is_some());
    }

    #[test]
    fn root_path_matches_empty_template() {
        let pattern = RoutePattern::parse("/");
        assert!(pattern.matches("/").is_some());
        assert!(pattern.matches("/x").is_none());
    }

    #[test]
    fn catch_all_captures_the_remaining_path() {
        let pattern = RoutePattern::parse("/tile/{*rest}");
        let params = pattern
            .matches("/tile/8/0/001.p/5")
            .expect("catch-all matches a variable-depth path");
        assert_eq!(params.get("rest"), Some("8/0/001.p/5"));
    }

    #[test]
    fn catch_all_matches_a_single_trailing_segment() {
        let pattern = RoutePattern::parse("/tile/{*rest}");
        assert_eq!(
            pattern
                .matches("/tile/checkpoint")
                .expect("matches")
                .get("rest"),
            Some("checkpoint")
        );
    }

    #[test]
    fn catch_all_requires_the_prefix_to_match() {
        let pattern = RoutePattern::parse("/tile/{*rest}");
        assert!(pattern.matches("/other/8/0").is_none());
    }

    #[test]
    fn a_normal_param_still_matches_exactly_one_segment() {
        let pattern = RoutePattern::parse("/users/{id}");
        assert!(pattern.matches("/users/1/2").is_none());
        assert_eq!(
            pattern.matches("/users/1").expect("matches").get("id"),
            Some("1")
        );
    }

    // The `pages/` scanner and this parser must share one template vocabulary.
    #[test]
    fn a_scanner_emitted_template_matches_a_real_path() {
        use fundamental_build::RouteTree;

        let dir = tempfile::TempDir::new().expect("temp pages dir");
        for rel in [
            "users/[id].rs",
            "blog/[...slug].rs",
            "[org]/projects/[project_id].rs",
        ] {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().expect("page has a parent"))
                .expect("create page dir");
            std::fs::write(&path, "pub fn page() {}").expect("write page");
        }
        let tree = RouteTree::scan(dir.path()).expect("scan the fixture tree");

        let matched = |template: &str, path: &str| RoutePattern::parse(template).matches(path);

        for route in &tree.routes {
            let pattern = RoutePattern::parse(&route.url_path);
            for param in &route.params {
                assert!(
                    pattern.segments.iter().any(|segment| matches!(
                        segment,
                        Segment::Param(name) | Segment::CatchAll(name) if name == param
                    )),
                    "scanned template {} does not capture {param} — it parsed as a literal",
                    route.url_path
                );
            }
        }

        let users = matched("/users/{id}", "/users/42").expect("a scanned dynamic route matches");
        assert_eq!(users.get("id"), Some("42"));

        let nested = matched("/{org}/projects/{project_id}", "/acme/projects/7")
            .expect("a route parameterized by its directories matches");
        assert_eq!(nested.get("org"), Some("acme"));
        assert_eq!(nested.get("project_id"), Some("7"));

        let blog = matched("/blog/{*slug}", "/blog/2026/08/hello").expect("a catch-all matches");
        assert_eq!(blog.get("slug"), Some("2026/08/hello"));
    }
}
