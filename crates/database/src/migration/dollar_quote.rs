//! Reading PostgreSQL's dollar-quoted strings, so a splitter does not cut a function body
//! at its inner `;`.

use std::iter::Peekable;
use std::str::Chars;

/// Reads a dollar-quote opener, the leading `$` having just been consumed.
///
/// `Ok(tag)` is the whole opener (`$$`, `$fn$`); `Err(text)` is what was read, all tag-legal
/// characters, when none opens here, as for a `$1` placeholder.
pub(crate) fn take_dollar_tag(chars: &mut Peekable<Chars<'_>>) -> Result<String, String> {
    let mut read = String::from('$');
    let mut body = String::new();
    while let Some(&c) = chars.peek() {
        if c == '$' {
            read.push(chars.next().expect("peeked"));
            return if is_tag_body(&body) {
                Ok(read)
            } else {
                Err(read)
            };
        }
        if c.is_alphanumeric() || c == '_' {
            let c = chars.next().expect("peeked");
            read.push(c);
            body.push(c);
        } else {
            break;
        }
    }
    Err(read)
}

/// Copies through to the end of an open dollar quote, `tag` included.
///
/// An unterminated quote consumes the rest of the input; the engine reports the malformed text.
pub(crate) fn copy_to_closing_tag(chars: &mut Peekable<Chars<'_>>, out: &mut String, tag: &str) {
    for c in chars.by_ref() {
        out.push(c);
        if c == '$' && out.ends_with(tag) {
            return;
        }
    }
}

/// Whether the text between two `$` is a legal tag: empty, or an identifier.
fn is_tag_body(body: &str) -> bool {
    let mut chars = body.chars();
    match chars.next() {
        None => true,
        Some(first) => {
            (first.is_alphabetic() || first == '_')
                && chars.all(|c| c.is_alphanumeric() || c == '_')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(input: &str) -> Result<String, String> {
        let mut chars = input.chars().peekable();
        assert_eq!(chars.next(), Some('$'), "the fixture feeds the leading `$`");
        take_dollar_tag(&mut chars)
    }

    #[test]
    fn the_anonymous_tag_and_a_named_one_both_open() {
        assert_eq!(take("$$ body $$"), Ok("$$".to_owned()));
        assert_eq!(take("$fn$ body $fn$"), Ok("$fn$".to_owned()));
        assert_eq!(take("$_x1$ body $_x1$"), Ok("$_x1$".to_owned()));
    }

    #[test]
    fn a_bind_placeholder_is_not_a_quote_and_is_handed_back_whole() {
        // `$1` must not open a quote that never closes.
        assert_eq!(take("$1, $2)"), Err("$1".to_owned()));
        assert_eq!(
            take("$12$"),
            Err("$12$".to_owned()),
            "a tag may not start with a digit"
        );
        assert_eq!(take("$ 1"), Err("$".to_owned()));
    }

    #[test]
    fn what_is_handed_back_never_contains_a_separator() {
        for input in ["$1, $2", "$12$", "$ ;", "$"] {
            let Err(text) = take(input) else { continue };
            assert!(
                !text.contains(';') && !text.contains(','),
                "a failed tag scan must not have swallowed a separator: {text:?}"
            );
        }
    }

    #[test]
    fn copying_stops_at_the_matching_tag_and_not_a_different_one() {
        let mut chars = " outer $inner$ nested $inner$ still outer $out$ after"
            .chars()
            .peekable();
        let mut out = String::from("$out$");
        copy_to_closing_tag(&mut chars, &mut out, "$out$");
        assert!(out.ends_with("$out$"));
        assert!(
            out.contains("$inner$ nested $inner$"),
            "a different tag must not close it"
        );
        assert_eq!(chars.collect::<String>(), " after");
    }
}
