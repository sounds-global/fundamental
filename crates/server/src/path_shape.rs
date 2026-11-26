use std::fmt;

use crate::HttpContext;

/// How many segments are rendered before the rest is elided.
const MAX_SEGMENTS: usize = 8;

/// A request path reduced to what the server wrote: declared segments verbatim, every other as its class and length.
///
/// ```text
/// /reset/jan@example.nl          →  /reset/{email}
/// /orders/0612345678/invoice     →  /orders/{number:9-16}/invoice
/// /wp-admin/setup.php?x=1        →  /{text:1-8}/{text:9-16}
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct PathShape(String);

impl PathShape {
    /// The shape of the route a handler is serving: its template; `/{unrouted}` without one.
    #[must_use]
    pub fn of_route<S>(ctx: &HttpContext<S>) -> Self {
        match ctx.route_template() {
            Some(template) => Self::new(template, |_| true),
            None => Self(String::from("/{unrouted}")),
        }
    }

    /// Shapes `path`, keeping only the segments `declared` recognises; any query is dropped first.
    #[must_use]
    pub(crate) fn new(path: &str, declared: impl Fn(&str) -> bool) -> Self {
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        let mut shape = String::with_capacity(64);
        let mut segments = path.strip_prefix('/').unwrap_or(path).split('/');
        for segment in segments.by_ref().take(MAX_SEGMENTS) {
            shape.push('/');
            if segment.is_empty() || declared(segment) {
                shape.push_str(segment);
            } else {
                shape.push_str(placeholder(segment));
            }
        }
        if segments.next().is_some() {
            shape.push_str("/…");
        }
        Self(shape)
    }

    /// The shape, ready to record.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PathShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for PathShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// The class and length bucket of a segment the server did not declare.
fn placeholder(segment: &str) -> &'static str {
    let bytes = segment.as_bytes();
    if segment.contains('@') || contains_ignore_case(bytes, b"%40") {
        return "{email}";
    }
    let class = if bytes.iter().all(u8::is_ascii_digit) {
        Class::Number
    } else if bytes.iter().all(u8::is_ascii_hexdigit) {
        Class::Hex
    } else if bytes.iter().all(|&b| is_unreserved(b)) {
        Class::Text
    } else {
        return "{encoded}";
    };
    class.bucket(bytes.len())
}

/// The RFC 3986 unreserved set.
fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn contains_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

#[derive(Clone, Copy)]
enum Class {
    Number,
    Hex,
    Text,
}

impl Class {
    fn bucket(self, len: usize) -> &'static str {
        let bucket = match len {
            0..=8 => 0,
            9..=16 => 1,
            17..=32 => 2,
            _ => 3,
        };
        match self {
            Self::Number => [
                "{number:1-8}",
                "{number:9-16}",
                "{number:17-32}",
                "{number:33+}",
            ][bucket],
            Self::Hex => ["{hex:1-8}", "{hex:9-16}", "{hex:17-32}", "{hex:33+}"][bucket],
            Self::Text => ["{text:1-8}", "{text:9-16}", "{text:17-32}", "{text:33+}"][bucket],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(segment: &str) -> bool {
        matches!(segment, "reset" | "orders" | "invoice" | "nl" | "users")
    }

    fn shape(path: &str) -> String {
        PathShape::new(path, declared).to_string()
    }

    #[test]
    fn declared_segments_are_kept_as_written() {
        assert_eq!(shape("/orders/invoice"), "/orders/invoice");
        assert_eq!(shape("/"), "/");
        assert_eq!(shape("/nl//users"), "/nl//users");
    }

    #[test]
    fn an_email_address_is_classed_in_either_spelling() {
        assert_eq!(shape("/reset/jan@example.nl"), "/reset/{email}");
        assert_eq!(shape("/reset/jan%40example.nl"), "/reset/{email}");
        assert_eq!(shape("/reset/jan%40EXAMPLE.nl"), "/reset/{email}");
    }

    /// The values a deny-list would have passed: none has a sensitive shape.
    #[test]
    fn personal_values_without_a_shape_are_classed_too() {
        assert_eq!(shape("/users/0612345678"), "/users/{number:9-16}");
        assert_eq!(shape("/users/06-12-34-56-78"), "/users/{text:9-16}");
        assert_eq!(shape("/users/1990-05-17"), "/users/{text:9-16}");
        assert_eq!(shape("/users/1234AB"), "/users/{hex:1-8}");
        assert_eq!(shape("/users/AB-123-C"), "/users/{text:1-8}");
        assert_eq!(shape("/users/jan"), "/users/{text:1-8}");
        assert_eq!(
            shape("/users/correct-horse-battery-staple"),
            "/users/{text:17-32}"
        );
    }

    #[test]
    fn tokens_are_classed_by_length() {
        let token = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert_eq!(shape(&format!("/reset/{token}")), "/reset/{hex:33+}");
        assert_eq!(
            shape("/users/67e55044-10b1-426f-9247-bb680e5fe0c8"),
            "/users/{text:33+}"
        );
    }

    #[test]
    fn encoded_and_control_bytes_are_one_class() {
        assert_eq!(shape("/users/j%C3%A9"), "/users/{encoded}");
        assert_eq!(shape("/users/a\u{1b}[2J\nb"), "/users/{encoded}");
        assert_eq!(shape("/users/jé"), "/users/{encoded}");
        assert_eq!(shape("/users/a;b=c"), "/users/{encoded}");
        assert_eq!(shape("/users/a+b"), "/users/{encoded}");
    }

    #[test]
    fn the_query_is_never_part_of_the_shape() {
        assert_eq!(shape("/reset?email=jan@example.nl"), "/reset");
    }

    #[test]
    fn a_deep_path_is_cut_after_eight_segments() {
        let deep = "/orders".repeat(20);
        assert_eq!(shape(&deep), format!("{}/…", "/orders".repeat(8)));
    }

    #[test]
    fn an_oversized_path_renders_bounded() {
        let huge = format!("/{}", "z".repeat(64 * 1024));
        let rendered = shape(&huge);
        assert_eq!(rendered, "/{text:33+}");
    }

    #[test]
    fn a_route_s_shape_is_its_template() {
        let routed: HttpContext<()> =
            HttpContext::new((), "req-1").with_route_template("/witness/{name}".into());
        assert_eq!(PathShape::of_route(&routed).as_str(), "/witness/{name}");
        let unrouted: HttpContext<()> = HttpContext::new((), "req-1");
        assert_eq!(PathShape::of_route(&unrouted).as_str(), "/{unrouted}");
    }

    #[test]
    fn nothing_is_declared_by_default_so_nothing_is_kept() {
        let shaped = PathShape::new("/reset/jan", |_| false);
        assert_eq!(shaped.as_str(), "/{text:1-8}/{text:1-8}");
        assert_eq!(format!("{shaped:?}"), "\"/{text:1-8}/{text:1-8}\"");
    }
}
