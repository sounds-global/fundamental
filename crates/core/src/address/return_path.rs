//! [`ReturnPath`] — a same-origin path safe to put in a `Location`.

#[cfg(not(feature = "std"))]
use alloc::{borrow::ToOwned, format, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

use super::{RequestAddress, ReturnPathError, percent_encode_segment};

/// Longer than any route an application serves, short enough for a header or a log line.
const LIMIT: usize = 512;

/// A same-origin path this application actually serves, safe to put in a `Location`.
///
/// Built only from a matched route ([`from_route`](Self::from_route)) or a validated
/// [`parse`](Self::parse), which deserialization also goes through, so an open redirect
/// cannot be constructed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReturnPath(String);

impl TryFrom<String> for ReturnPath {
    type Error = ReturnPathError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<ReturnPath> for String {
    fn from(path: ReturnPath) -> Self {
        path.0
    }
}

impl ReturnPath {
    /// The longest return path [`parse`](Self::parse) accepts, in bytes.
    pub const LIMIT: usize = LIMIT;

    /// Rebuild the path a request was for, from the route it matched.
    ///
    /// `None` when the rebuilt path exceeds [`LIMIT`](Self::LIMIT).
    #[must_use]
    pub fn from_route<'a>(
        template: &str,
        lookup: impl Fn(&str) -> Option<&'a str>,
        query: &str,
    ) -> Option<Self> {
        let mut path = String::with_capacity(template.len());
        for segment in template.split('/').skip(1) {
            path.push('/');
            match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                // Encoded, so a captured `../..` or `a?b` cannot restructure the URL.
                Some(name) => {
                    path.push_str(&percent_encode_segment(lookup(name).unwrap_or_default()))
                }
                None => path.push_str(segment),
            }
        }
        if !query.is_empty() {
            path.push('?');
            path.push_str(query);
        }
        (path.len() <= LIMIT).then_some(Self(path))
    }

    /// Read a `?next=` value back, refusing every shape that is not a same-origin
    /// path.
    ///
    /// # Errors
    /// A [`ReturnPathError`] naming which shape arrived.
    pub fn parse(raw: &str) -> Result<Self, ReturnPathError> {
        if raw.len() > LIMIT {
            return Err(ReturnPathError::TooLong { limit: LIMIT });
        }
        if raw
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        {
            return Err(ReturnPathError::NotAPath);
        }
        if !raw.starts_with('/') {
            return Err(ReturnPathError::MustBeAbsolutePath);
        }
        if raw.contains('#') {
            return Err(ReturnPathError::Fragment);
        }
        // Percent-encoded forms too: a browser normalises them before following the redirect.
        let rest = &raw[1..];
        let lowered = rest.to_ascii_lowercase();
        if rest.starts_with('/') || lowered.starts_with("%2f") || lowered.starts_with("%5c") {
            return Err(ReturnPathError::ProtocolRelative);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Read the `name` parameter out of a raw query string, and parse it.
    ///
    /// `Ok(None)` when the parameter is absent; `+` is not read as a space.
    ///
    /// # Errors
    /// [`ReturnPathError::MalformedEncoding`] for an escape that does not decode, and
    /// whatever [`parse`](Self::parse) refuses for one that does.
    pub fn from_query(query: &str, name: &str) -> Result<Option<Self>, ReturnPathError> {
        let Some(raw) = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| (key == name).then_some(value))
        else {
            return Ok(None);
        };
        let decoded = decode(raw).ok_or(ReturnPathError::MalformedEncoding)?;
        Self::parse(&decoded).map(Some)
    }

    /// This path with every `name` query parameter removed.
    ///
    /// Matching is on the whole key, so `?language=nl` survives `without_query_param("lang")`.
    #[must_use]
    pub fn without_query_param(self, name: &str) -> Self {
        self.without_query_params(|key| key == name)
    }

    /// This path without every query parameter whose key `remove` answers `true` for.
    #[must_use]
    pub fn without_query_params(self, remove: impl Fn(&str) -> bool) -> Self {
        let Some((path, query)) = self.0.split_once('?') else {
            return self;
        };
        let kept: Vec<&str> = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .filter(|pair| !remove(pair.split_once('=').map_or(*pair, |(key, _)| key)))
            .collect();
        Self(match kept.is_empty() {
            // Not `{path}?`: a trailing `?` is a second address for one page.
            true => path.to_owned(),
            false => format!("{path}?{}", kept.join("&")),
        })
    }

    /// The value, ready for a `Location` header. Always begins with a single `/`.
    #[must_use]
    pub fn as_location(&self) -> &str {
        &self.0
    }

    /// This path as `address` serves it: under the reader's prefix, spelled in its language,
    /// the query kept byte for byte.
    #[must_use]
    pub fn served_at(&self, address: &RequestAddress) -> Self {
        Self::parse(&address.href(&self.0)).unwrap_or_else(|_| self.clone())
    }

    /// The value, encoded for use as a query parameter.
    #[must_use]
    pub fn encoded(&self) -> String {
        percent_encode_segment(&self.0).into_owned()
    }
}

/// Percent-decode a query parameter's value, or `None` for any malformed input.
fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = value.get(index + 1..index + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, ReturnPath};
    use crate::address::ReturnPathError;

    /// The prefix and spelling come from the address space; the query arrives as asked.
    #[test]
    fn a_return_path_is_served_where_the_reader_was_with_its_query_kept() {
        use crate::address::{
            AddressSpace, LocalePrefixSet, PageSlug, PrefixScope, RequestAddress, SlugTable,
        };
        #[cfg(not(feature = "std"))]
        use alloc::sync::Arc;
        #[cfg(feature = "std")]
        use std::sync::Arc;

        let prefixes = LocalePrefixSet::new()
            .with_paths_in("nl", "nl-NL".parse().expect("tag"), PrefixScope::Full, "en")
            .expect("nl");
        let slugs = SlugTable::builder()
            .page("/aanvragen", &[PageSlug::current("en", "requests")])
            .page("/aanvragen/{build}", &[])
            .build()
            .expect("well formed");
        let space = Arc::new(
            AddressSpace::new(prefixes, slugs)
                .with_default_language("en".parse().expect("tag"))
                .expect("default"),
        );
        let canonical = ReturnPath::from_route("/aanvragen/{build}", |_| Some("7"), "deel=a%2fb")
            .expect("short");
        let dutch = RequestAddress::new(
            Arc::clone(&space),
            "nl-NL".parse().expect("tag"),
            "/nl",
            "/aanvragen/{build}",
        );
        assert_eq!(
            canonical.served_at(&dutch).as_location(),
            "/nl/requests/7?deel=a%2fb"
        );
        let english =
            RequestAddress::new(space, "en".parse().expect("tag"), "", "/aanvragen/{build}");
        assert_eq!(
            canonical.served_at(&english).as_location(),
            "/requests/7?deel=a%2fb"
        );
    }

    /// No caller string reaches the output; a captured value is encoded.
    #[test]
    fn a_rebuilt_path_encodes_what_a_route_captured() {
        let rebuilt = ReturnPath::from_route(
            "/clients/{client_id}",
            |name| (name == "client_id").then_some("../../evil"),
            "tab=files",
        )
        .expect("within the bound");
        assert_eq!(
            rebuilt.as_location(),
            "/clients/..%2F..%2Fevil?tab=files",
            "a captured value must not restructure the path it lands in"
        );
    }

    /// An uncaptured parameter leaves an empty segment, never a literal `{name}`.
    #[test]
    fn an_uncaptured_parameter_is_empty_rather_than_literal() {
        let rebuilt =
            ReturnPath::from_route("/clients/{client_id}", |_| None, "").expect("within the bound");
        assert_eq!(rebuilt.as_location(), "/clients/");
    }

    #[test]
    fn a_rebuilt_path_longer_than_the_bound_is_refused() {
        assert_eq!(
            ReturnPath::from_route("/x", |_| None, &"a".repeat(LIMIT)),
            None
        );
    }

    /// The shapes an open redirect wears.
    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn every_shape_that_is_not_a_same_origin_path_is_refused() {
        assert!(matches!(
            ReturnPath::parse("clients"),
            Err(ReturnPathError::MustBeAbsolutePath)
        ));
        assert!(matches!(
            ReturnPath::parse("//evil.test"),
            Err(ReturnPathError::ProtocolRelative)
        ));
        assert!(matches!(
            ReturnPath::parse("/%2f%2fevil.test"),
            Err(ReturnPathError::ProtocolRelative)
        ));
        assert!(matches!(
            ReturnPath::parse("/%5cevil.test"),
            Err(ReturnPathError::ProtocolRelative)
        ));
        assert!(matches!(
            ReturnPath::parse("/a\rb"),
            Err(ReturnPathError::NotAPath)
        ));
        assert!(matches!(
            ReturnPath::parse("/a\\b"),
            Err(ReturnPathError::NotAPath)
        ));
        assert!(matches!(
            ReturnPath::parse("https://evil.test"),
            Err(ReturnPathError::MustBeAbsolutePath)
        ));
        assert!(matches!(
            ReturnPath::parse(&format!("/{}", "a".repeat(LIMIT))),
            Err(ReturnPathError::TooLong { .. })
        ));
        assert!(matches!(
            ReturnPath::parse("/aanmelden#code=7K2Q-M9XD"),
            Err(ReturnPathError::Fragment)
        ));
    }

    #[test]
    fn a_same_origin_path_is_accepted_unchanged() {
        for raw in ["/", "/clients", "/clients/42?tab=files", "/why?lang=nl"] {
            let parsed = ReturnPath::parse(raw).unwrap_or_else(|_| panic!("`{raw}` is a path"));
            assert_eq!(parsed.as_location(), raw);
        }
    }

    /// A rebuilt path must survive its own parser.
    #[test]
    fn what_the_rebuild_produces_the_parser_accepts() {
        let rebuilt = ReturnPath::from_route(
            "/clients/{client_id}",
            |_| Some("../../evil"),
            "tab=files&lang=nl",
        )
        .expect("within the bound");
        assert!(ReturnPath::parse(rebuilt.as_location()).is_ok());
    }

    #[test]
    fn a_named_query_parameter_is_removed_and_its_lookalike_is_not() {
        let stripped = ReturnPath::parse("/why?language=nl&lang=nl&tab=x")
            .expect("a path")
            .without_query_param("lang");
        assert_eq!(stripped.as_location(), "/why?language=nl&tab=x");
    }

    #[test]
    fn removing_the_only_parameter_leaves_no_question_mark() {
        let stripped = ReturnPath::parse("/why?lang=nl")
            .expect("a path")
            .without_query_param("lang");
        assert_eq!(stripped.as_location(), "/why");
    }

    #[test]
    fn a_bare_key_with_no_value_is_still_that_parameter() {
        let stripped = ReturnPath::parse("/why?lang")
            .expect("a path")
            .without_query_param("lang");
        assert_eq!(stripped.as_location(), "/why");
    }

    /// What the page gate writes is what the sign-in page reads back.
    #[test]
    fn what_the_encoder_writes_the_query_reader_recovers() {
        for raw in ["/", "/clients", "/clients/42?tab=files&lang=nl", "/a+b"] {
            let path = ReturnPath::parse(raw).expect("a path");
            let query = format!("next={}", path.encoded());
            assert_eq!(
                ReturnPath::from_query(&query, "next"),
                Ok(Some(path)),
                "`{raw}` did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_destination_is_found_beside_other_parameters() {
        assert_eq!(
            ReturnPath::from_query("lang=nl&next=%2Fclients&refused=1", "next"),
            Ok(ReturnPath::parse("/clients").ok())
        );
    }

    /// A `?next=` that is absent is no destination and no refusal.
    #[test]
    fn an_absent_destination_is_none() {
        for query in ["", "lang=nl", "next", "nextish=%2Fclients"] {
            assert_eq!(
                ReturnPath::from_query(query, "next"),
                Ok(None),
                "`{query}` produced a destination or a refusal"
            );
        }
    }

    /// Every way a present `?next=` fails to be a destination is refused, and says which.
    #[test]
    fn every_unusable_destination_names_its_refusal() {
        for (query, refusal) in [
            ("next=", ReturnPathError::MustBeAbsolutePath),
            ("next=%2", ReturnPathError::MalformedEncoding),
            ("next=%zz", ReturnPathError::MalformedEncoding),
            ("next=%FF%FE", ReturnPathError::MalformedEncoding),
            (
                "next=https%3A%2F%2Fevil.test",
                ReturnPathError::MustBeAbsolutePath,
            ),
            ("next=%2F%2Fevil.test", ReturnPathError::ProtocolRelative),
            ("next=%2Fa%0Db", ReturnPathError::NotAPath),
            ("next=%2Fa%23code%3DX", ReturnPathError::Fragment),
        ] {
            assert_eq!(
                ReturnPath::from_query(query, "next"),
                Err(refusal),
                "`{query}`"
            );
        }
    }

    /// A body carrying an off-origin value does not deserialize at all.
    #[test]
    fn an_off_origin_value_does_not_deserialize() {
        for hostile in [
            "\"https://evil.test\"",
            "\"//evil.test\"",
            "\"/%2f%2fevil.test\"",
            "\"clients\"",
        ] {
            assert!(
                serde_json::from_str::<ReturnPath>(hostile).is_err(),
                "`{hostile}` deserialized into a return path"
            );
        }
    }

    #[test]
    fn a_same_origin_value_round_trips_as_a_plain_string() {
        let parsed: ReturnPath =
            serde_json::from_str("\"/clients/42?tab=files\"").expect("a same-origin path");
        assert_eq!(parsed.as_location(), "/clients/42?tab=files");
        assert_eq!(
            serde_json::to_string(&parsed).expect("serialise"),
            "\"/clients/42?tab=files\"",
            "the wire form is the path itself, not an object wrapping it"
        );
    }

    #[test]
    fn the_encoded_form_is_safe_to_put_in_a_query() {
        let encoded = ReturnPath::parse("/clients/42?tab=files")
            .expect("a path")
            .encoded();
        assert_eq!(encoded, "%2Fclients%2F42%3Ftab%3Dfiles");
    }
}
