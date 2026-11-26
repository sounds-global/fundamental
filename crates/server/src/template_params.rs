use crate::route_pattern::RoutePattern;

/// The names a route template captures, in path order, read by the router's own parser.
#[must_use]
pub fn template_params(template: &str) -> Vec<String> {
    RoutePattern::parse(template)
        .params()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::template_params;

    #[test]
    fn captures_and_catch_alls_in_path_order() {
        assert_eq!(
            template_params("/teams/{team}/files/{*rest}"),
            ["team", "rest"]
        );
        assert!(template_params("/").is_empty());
        assert!(template_params("/about/").is_empty());
    }
}
