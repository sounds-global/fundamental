//! [`propose_slug`] — a slug made from a name, in any script.

#[cfg(not(feature = "std"))]
use alloc::{borrow::ToOwned, string::String, vec, vec::Vec};

use crate::address::Slug;

/// The longest proposal, leaving room for a `-2` … `-9` suffix within [`Slug::MAX_LEN`].
const PROPOSAL_LEN: usize = 48;

/// A slug made from `name` — a legal name, a title — in the language it is written in.
///
/// Transliterated to ASCII, lowercased, every other run turned into one hyphen and trimmed
/// at a hyphen; `None` when nothing readable is left.
#[must_use]
pub fn propose_slug(name: &str, language: Option<&str>) -> Option<Slug> {
    let folded: String = match language {
        Some("de") => name
            .chars()
            .flat_map(|c| match c {
                'ä' => "ae".chars().collect::<Vec<_>>(),
                'ö' => "oe".chars().collect(),
                'ü' => "ue".chars().collect(),
                'Ä' => "Ae".chars().collect(),
                'Ö' => "Oe".chars().collect(),
                'Ü' => "Ue".chars().collect(),
                other => vec![other],
            })
            .collect(),
        _ => name.to_owned(),
    };
    let ascii = deunicode::deunicode(&folded).to_ascii_lowercase();
    let mut slug = String::with_capacity(ascii.len());
    for c in ascii.chars() {
        match c {
            'a'..='z' | '0'..='9' => slug.push(c),
            _ if !slug.is_empty() && !slug.ends_with('-') => slug.push('-'),
            _ => {}
        }
    }
    while slug.len() > PROPOSAL_LEN {
        match slug[..PROPOSAL_LEN].rfind('-') {
            Some(at) if at > 0 => slug.truncate(at),
            _ => slug.truncate(PROPOSAL_LEN),
        }
    }
    let slug = slug.trim_end_matches('-');
    Slug::parse(slug).ok()
}

#[cfg(test)]
mod tests {
    use super::propose_slug;

    fn proposed(name: &str, language: Option<&str>) -> Option<String> {
        propose_slug(name, language).map(|slug| slug.to_string())
    }

    #[test]
    fn proposal_transliterates_e_acute_and_sharp_s() {
        assert_eq!(
            proposed("Café Straße B.V.", None).as_deref(),
            Some("cafe-strasse-b-v")
        );
        assert_eq!(
            proposed("Acme Books B.V.", Some("nl")).as_deref(),
            Some("acme-books-b-v")
        );
    }

    #[test]
    fn german_folds_umlauts_the_german_way() {
        assert_eq!(
            proposed("Müller & Söhne", Some("de")).as_deref(),
            Some("mueller-soehne")
        );
        assert_eq!(
            proposed("Müller & Söhne", Some("nl")).as_deref(),
            Some("muller-sohne")
        );
    }

    #[test]
    fn any_script_becomes_readable() {
        assert_eq!(proposed("Москва", None).as_deref(), Some("moskva"));
    }

    #[test]
    fn a_long_name_is_cut_at_a_hyphen_and_nothing_readable_is_none() {
        let long = proposed(&"word ".repeat(30), None).unwrap_or_default();
        assert!(long.len() <= 48 && !long.ends_with('-'), "{long}");
        assert_eq!(proposed("!!!", None), None);
        assert_eq!(proposed("1234", None), None, "a number is not a name");
    }
}
