//! Path-derived PascalCase component naming, unique per route.

/// A PascalCase component name derived from a route's URL path, e.g. `/users/{id}` -> `UsersIdPage`.
pub(crate) fn component_name(url_path: &str) -> String {
    let basis = name_basis(url_path);
    let pascal = to_pascal_case(&basis);
    format!("{pascal}Page")
}

/// A snake_case identifier basis derived from a route's URL path, e.g. `/users/{id}` -> `users_id`.
pub(crate) fn snake_basis(url_path: &str) -> String {
    name_basis(url_path)
        .split(['_', '-', '/'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn name_basis(url_path: &str) -> String {
    let cleaned: Vec<String> = url_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(strip_markers)
        .filter(|s| !s.is_empty())
        .collect();

    if cleaned.is_empty() {
        "index".to_string()
    } else {
        cleaned.join("/")
    }
}

/// Remove the routing markers `[`, `]`, `{`, `}`, `.`, `:`, `*` from a segment.
fn strip_markers(segment: &str) -> String {
    segment
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | '{' | '}' | '.' | ':' | '*'))
        .collect()
}

fn to_pascal_case(basis: &str) -> String {
    basis
        .split(['_', '-', '/'])
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_index_page() {
        assert_eq!(component_name("/"), "IndexPage");
        assert_eq!(snake_basis("/"), "index");
    }

    #[test]
    fn static_route() {
        assert_eq!(component_name("/about"), "AboutPage");
        assert_eq!(snake_basis("/about"), "about");
    }

    #[test]
    fn dynamic_route_uses_full_path() {
        assert_eq!(component_name("/users/{id}"), "UsersIdPage");
        assert_eq!(snake_basis("/users/{id}"), "users_id");
    }

    #[test]
    fn catch_all_route() {
        assert_eq!(component_name("/blog/{*slug}"), "BlogSlugPage");
        assert_eq!(snake_basis("/blog/{*slug}"), "blog_slug");
    }

    #[test]
    fn a_capture_yields_an_identifier_not_a_brace() {
        for name in [
            component_name("/users/{id}"),
            component_name("/blog/{*slug}"),
        ] {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric()),
                "{name} is not a bare identifier"
            );
        }
    }

    #[test]
    fn both_marker_vocabularies_name_the_same_component() {
        assert_eq!(component_name("/users/:id"), component_name("/users/{id}"));
        assert_eq!(snake_basis("/blog/*slug"), snake_basis("/blog/{*slug}"));
    }

    #[test]
    fn snake_and_kebab_segments_split() {
        assert_eq!(component_name("/user_profile"), "UserProfilePage");
        assert_eq!(component_name("/user-profile"), "UserProfilePage");
        assert_eq!(snake_basis("/user-profile"), "user_profile");
    }

    #[test]
    fn nested_index_does_not_collide() {
        assert_ne!(component_name("/users"), component_name("/posts"));
        assert_eq!(component_name("/users"), "UsersPage");
        assert_eq!(component_name("/posts"), "PostsPage");
    }
}
