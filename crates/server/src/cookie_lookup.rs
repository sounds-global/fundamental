/// What reading a named cookie out of a request's `Cookie` fields found.
///
/// Two different values is ambiguous rather than present: a sibling subdomain can place a
/// shadow ahead of the real cookie (RFC 6265 §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookieLookup<'a> {
    /// No pair with that name carried a value.
    Absent,
    /// Exactly one distinct value was present.
    Found(&'a str),
    /// The name was present with this many *distinct* values.
    Ambiguous(usize),
}

impl<'a> CookieLookup<'a> {
    /// Reads `name` out of every `Cookie` field (RFC 6265 §4.2.1), case-sensitively.
    ///
    /// A quoted value is returned as written, and an empty value counts as absent.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) fn find(fields: impl Iterator<Item = &'a str>, name: &str) -> Self {
        let mut values: Vec<&'a str> = Vec::new();
        for field in fields {
            for pair in field.split(';') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                if key.trim() != name {
                    continue;
                }
                let value = value.trim();
                if !value.is_empty() && !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        match values.as_slice() {
            [] => Self::Absent,
            [only] => Self::Found(only),
            many => Self::Ambiguous(many.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(fields: &[&'a str], name: &str) -> CookieLookup<'a> {
        CookieLookup::find(fields.iter().copied(), name)
    }

    #[test]
    fn finds_a_single_pair() {
        assert_eq!(
            find(&["session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn finds_a_pair_among_others() {
        assert_eq!(
            find(&["theme=dark; session=abc; locale=nl"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn the_name_is_matched_case_sensitively() {
        assert_eq!(find(&["Session=abc"], "session"), CookieLookup::Absent);
        assert_eq!(find(&["SESSION=abc"], "session"), CookieLookup::Absent);
    }

    /// A name that merely contains the needle is a different cookie entirely.
    #[test]
    fn a_name_is_matched_whole_not_by_substring() {
        assert_eq!(find(&["xsession=abc"], "session"), CookieLookup::Absent);
        assert_eq!(find(&["session_id=abc"], "session"), CookieLookup::Absent);
        assert_eq!(find(&["presession=abc"], "session"), CookieLookup::Absent);
    }

    #[test]
    fn optional_whitespace_is_trimmed_on_both_sides() {
        assert_eq!(
            find(&["a=1;  session = abc ; b=2"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn pairs_need_no_space_after_the_separator() {
        assert_eq!(
            find(&["a=1;session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    // A JWT is base64url, which pads with `=`.
    #[test]
    fn an_equals_inside_the_value_survives() {
        assert_eq!(
            find(&["session=eyJhbGciOiJFZERTQSJ9.body.sig=="], "session"),
            CookieLookup::Found("eyJhbGciOiJFZERTQSJ9.body.sig==")
        );
    }

    #[test]
    fn a_pair_with_no_equals_is_skipped_without_aborting_the_scan() {
        assert_eq!(
            find(&["broken; session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    /// How a cleared cookie arrives. It must not mask a valid pair.
    #[test]
    fn an_empty_value_is_absent_and_does_not_mask_a_valid_pair() {
        assert_eq!(find(&["session="], "session"), CookieLookup::Absent);
        assert_eq!(
            find(&["session=; session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn a_value_in_a_second_cookie_field_is_found() {
        assert_eq!(
            find(&["theme=dark", "session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn duplicate_names_with_different_values_are_ambiguous() {
        assert_eq!(
            find(&["session=attacker; session=real"], "session"),
            CookieLookup::Ambiguous(2)
        );
        assert_eq!(
            find(&["session=a", "session=b", "session=c"], "session"),
            CookieLookup::Ambiguous(3)
        );
    }

    /// The same value twice is not an ambiguity — there is nothing to choose between.
    #[test]
    fn duplicate_names_with_identical_values_collapse() {
        assert_eq!(
            find(&["session=abc; session=abc"], "session"),
            CookieLookup::Found("abc")
        );
    }

    #[test]
    fn a_quoted_value_is_returned_verbatim() {
        assert_eq!(
            find(&["session=\"abc\""], "session"),
            CookieLookup::Found("\"abc\"")
        );
    }

    #[test]
    fn no_cookie_fields_is_absent() {
        assert_eq!(find(&[], "session"), CookieLookup::Absent);
        assert_eq!(find(&[""], "session"), CookieLookup::Absent);
    }
}
