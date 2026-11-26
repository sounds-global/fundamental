//! Shared identifier-casing helpers used by the derives.

/// `PascalCase` to `snake_case`; a run of capitals stays one word (`HttpURL` to `http_url`).
pub(crate) fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let prev_lower_or_digit =
                i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
            // The last capital of a run followed by lowercase starts a new word.
            let run_then_lower = i > 0
                && chars[i - 1].is_uppercase()
                && i + 1 < chars.len()
                && chars[i + 1].is_lowercase();
            if prev_lower_or_digit || run_then_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `PascalCase` to `dotted.case`, the NATS subject convention.
pub(crate) fn to_dotted_case(name: &str) -> String {
    to_snake_case(name).replace('_', ".")
}

#[cfg(test)]
mod tests {
    use super::{to_dotted_case, to_snake_case};

    #[test]
    fn dotted_case_uses_dot_separators() {
        assert_eq!(to_dotted_case("Events"), "events");
        assert_eq!(to_dotted_case("UserCreated"), "user.created");
        assert_eq!(to_dotted_case("OrderPlaced"), "order.placed");
    }

    #[test]
    fn snake_case_handles_the_documented_shapes() {
        assert_eq!(to_snake_case("Country"), "country");
        assert_eq!(to_snake_case("RenderTarget"), "render_target");
        assert_eq!(to_snake_case("Us"), "us");
        assert_eq!(to_snake_case("NlBe"), "nl_be");
        assert_eq!(to_snake_case("EnUs"), "en_us");
    }

    #[test]
    fn snake_case_handles_capital_runs_reasonably() {
        assert_eq!(to_snake_case("HttpURL"), "http_url");
        assert_eq!(to_snake_case("URLPath"), "url_path");
    }
}
