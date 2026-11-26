use fundamental_core::i18n::Locale;
use tracing::debug;

/// A quality value of `1`, in thousandths: the RFC 9110 §12.4.2 default.
const DEFAULT_QUALITY: u16 = 1000;

/// What one `Accept-Language` entry asks for; `*` is an explicit "anything you have".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanguagePreference {
    /// A concrete language tag.
    Tag(Locale),
    /// The wildcard `*`: any language the server offers.
    Any,
}

/// Reads a request's `Accept-Language` value into preferences, best first (RFC 9110 §12.5.4).
///
/// Drops `q=0`, a malformed weight, and a range that is not a language; keeps the primary
/// subtag of a tag with a script or variant.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn preferences(header: &str) -> Vec<LanguagePreference> {
    let mut ranked: Vec<(u16, LanguagePreference)> = Vec::new();

    for entry in header.split(',') {
        let mut parts = entry.split(';');
        let range = parts.next().unwrap_or_default().trim();
        if range.is_empty() {
            continue;
        }

        let Some(quality) = quality(parts) else {
            debug!(
                range_bytes = range.len(),
                "skipping an Accept-Language entry with an unreadable q parameter"
            );
            continue;
        };
        if quality == 0 {
            continue;
        }

        let preference = if range == "*" {
            LanguagePreference::Any
        } else {
            match language_range(range) {
                Some(locale) => LanguagePreference::Tag(locale),
                None => {
                    debug!(
                        range_bytes = range.len(),
                        "skipping an Accept-Language entry that is not a language tag"
                    );
                    continue;
                }
            }
        };

        ranked.push((quality, preference));
    }

    // A stable sort, so equal weights keep their written order (RFC 9110 §12.4.2).
    ranked.sort_by_key(|(quality, _)| core::cmp::Reverse(*quality));
    ranked
        .into_iter()
        .map(|(_, preference)| preference)
        .collect()
}

/// The `q` weight of one entry's parameters, in thousandths; `None` if present but unreadable.
fn quality<'a>(parameters: impl Iterator<Item = &'a str>) -> Option<u16> {
    let mut quality = DEFAULT_QUALITY;
    for parameter in parameters {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("q") {
            continue;
        }
        quality = qvalue(value.trim())?;
    }
    Some(quality)
}

/// Parses an RFC 9110 §12.4.2 qvalue into thousandths.
fn qvalue(text: &str) -> Option<u16> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let mut thousandths: u16 = 0;
    for (position, byte) in fraction.bytes().enumerate() {
        let place = match position {
            0 => 100,
            1 => 10,
            _ => 1,
        };
        thousandths += u16::from(byte - b'0') * place;
    }

    match whole {
        "0" => Some(thousandths),
        "1" if thousandths == 0 => Some(DEFAULT_QUALITY),
        _ => None,
    }
}

/// Reads one language range into a [`Locale`], keeping the language when subtags are not modelled.
fn language_range(range: &str) -> Option<Locale> {
    if let Ok(locale) = range.parse::<Locale>() {
        return Some(locale);
    }
    range.split(['-', '_']).next()?.parse::<Locale>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(header: &str) -> Vec<String> {
        preferences(header)
            .into_iter()
            .map(|preference| match preference {
                LanguagePreference::Tag(locale) => locale.to_string(),
                LanguagePreference::Any => "*".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_single_tag_is_the_only_preference() {
        assert_eq!(tags("nl"), ["nl"]);
    }

    #[test]
    fn a_region_subtag_is_kept_and_case_normalised() {
        assert_eq!(tags("nl-be"), ["nl-BE"]);
    }

    #[test]
    fn entries_are_ordered_by_descending_weight() {
        assert_eq!(tags("en;q=0.5, nl;q=0.9, de;q=0.1"), ["nl", "en", "de"]);
    }

    /// The header's own order is the tie-break, so these two are different requests.
    #[test]
    fn equal_weights_keep_the_written_order() {
        assert_eq!(tags("en, nl"), ["en", "nl"]);
        assert_eq!(tags("nl, en"), ["nl", "en"]);
    }

    #[test]
    fn a_missing_q_ranks_above_any_weighted_entry() {
        assert_eq!(tags("en;q=0.9, nl"), ["nl", "en"]);
    }

    #[test]
    fn the_wildcard_is_a_preference_in_its_written_position() {
        assert_eq!(tags("nl;q=0.9, *;q=0.5"), ["nl", "*"]);
        assert_eq!(tags("*"), ["*"]);
    }

    /// RFC 9110 §12.4.2: `q=0` means *not acceptable*. Ranking it would invert the ask.
    #[test]
    fn a_zero_weight_is_a_refusal_not_a_last_choice() {
        assert_eq!(tags("nl, en;q=0"), ["nl"]);
        assert_eq!(tags("en;q=0.000"), Vec::<String>::new());
    }

    #[test]
    fn an_unreadable_weight_drops_the_entry_without_aborting_the_scan() {
        assert_eq!(tags("en;q=high, nl"), ["nl"]);
        assert_eq!(tags("en;q=1.5, nl"), ["nl"]);
        assert_eq!(tags("en;q=0.1234, nl"), ["nl"]);
    }

    #[test]
    fn a_range_that_is_not_a_language_is_dropped() {
        assert_eq!(tags("1234, nl"), ["nl"]);
    }

    #[test]
    fn a_script_subtag_degrades_to_the_language_rather_than_being_dropped() {
        assert_eq!(tags("zh-Hans-CN"), ["zh"]);
    }

    #[test]
    fn whitespace_around_entries_and_parameters_is_tolerated() {
        assert_eq!(tags(" nl-BE ;  q = 0.8 , en "), ["en", "nl-BE"]);
    }

    #[test]
    fn an_empty_header_yields_no_preference() {
        assert_eq!(tags(""), Vec::<String>::new());
        assert_eq!(tags(" , , "), Vec::<String>::new());
    }

    #[test]
    fn a_parameter_other_than_q_is_ignored() {
        assert_eq!(tags("nl;foo=bar"), ["nl"]);
    }

    #[test]
    fn qvalue_covers_the_whole_grammar() {
        assert_eq!(qvalue("1"), Some(1000));
        assert_eq!(qvalue("1.0"), Some(1000));
        assert_eq!(qvalue("1.000"), Some(1000));
        assert_eq!(qvalue("0"), Some(0));
        assert_eq!(qvalue("0.5"), Some(500));
        assert_eq!(qvalue("0.05"), Some(50));
        assert_eq!(qvalue("0.005"), Some(5));
        assert_eq!(qvalue("1.001"), None);
        assert_eq!(qvalue("2"), None);
        assert_eq!(qvalue(""), None);
    }
}
