//! [`split_top_level`]: splitting a comma list without descending into it.

use crate::migration::dollar_quote::{copy_to_closing_tag, take_dollar_tag};

/// Splits `input` on commas that are **not** nested inside parentheses or a quoted
/// string, trimming each part.
pub(crate) fn split_top_level(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    current.push(c);
                }
                // Dollar-quoted: a `DEFAULT` or `CHECK` may carry one.
                '$' => match take_dollar_tag(&mut chars) {
                    Ok(tag) => {
                        current.push_str(&tag);
                        copy_to_closing_tag(&mut chars, &mut current, &tag);
                    }
                    Err(text) => current.push_str(&text),
                },
                '(' => {
                    depth += 1;
                    current.push(c);
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    current.push(c);
                }
                ',' if depth == 0 => {
                    parts.push(current.trim().to_owned());
                    current.clear();
                }
                _ => current.push(c),
            },
        }
    }

    let last = current.trim();
    if !last.is_empty() {
        parts.push(last.to_owned());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::split_top_level;

    #[test]
    fn a_check_constraints_commas_stay_inside_their_column() {
        let parts = split_top_level(
            "zone TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('a', 'b', 'c')), id TEXT",
        );

        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[1],
            "state TEXT NOT NULL CHECK (state IN ('a', 'b', 'c'))"
        );
    }

    #[test]
    fn a_composite_hash_component_stays_one_element() {
        let parts = split_top_level("(zone, grant_id) HASH, seq ASC");

        assert_eq!(parts, ["(zone, grant_id) HASH", "seq ASC"]);
    }

    #[test]
    fn a_comma_inside_a_string_literal_does_not_split() {
        let parts = split_top_level("a TEXT DEFAULT 'x, y', b TEXT");

        assert_eq!(parts, ["a TEXT DEFAULT 'x, y'", "b TEXT"]);
    }

    #[test]
    fn an_empty_list_yields_nothing() {
        assert!(split_top_level("   ").is_empty());
    }

    #[test]
    fn a_dollar_quoted_default_keeps_its_commas_inside_its_column() {
        let parts = split_top_level("id TEXT, note TEXT DEFAULT $t$a, b$t$, n INT");
        assert_eq!(parts.len(), 3, "{parts:#?}");
        assert_eq!(parts[1], "note TEXT DEFAULT $t$a, b$t$");
    }
}
