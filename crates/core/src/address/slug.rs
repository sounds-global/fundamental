//! [`Slug`] — a readable name in an address: lowercase letters, digits and single hyphens.

use core::fmt;

#[cfg(not(feature = "std"))]
use alloc::{
    borrow::{Cow, ToOwned},
    string::String,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::borrow::Cow;

use crate::address::PathSegment;
use crate::model::Spelled;
use crate::validate::{ValidationError, ValidationErrors};

/// A name in an address, `acme-books`: 2–63 characters of `a-z`, `0-9` and single hyphens,
/// with at least one letter, and never shaped like an id.
///
/// ```compile_fail
/// use fundamental_core::address::Slug;
///
/// // Upper case and a space: no slug, and no program.
/// const NOT_ONE: Slug = Slug::checked("Not One");
/// let _ = NOT_ONE;
/// ```
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(Cow<'static, str>);

impl Slug {
    /// The shortest slug.
    pub const MIN_LEN: usize = 2;
    /// The longest slug.
    pub const MAX_LEN: usize = 63;

    /// The slug `text`, checked while the program compiles when it is a constant's value.
    ///
    /// # Panics
    /// When `text` is no slug; in a constant that is a compile error.
    #[must_use]
    pub const fn checked(text: &'static str) -> Self {
        assert!(
            is_slug(text.as_bytes()),
            "a slug is 2-63 of a-z, 0-9 and single hyphens, with a letter, never shaped like an id"
        );
        Self(Cow::Borrowed(text))
    }

    /// Read the one spelling, collecting every problem.
    ///
    /// # Errors
    ///
    /// Every problem under the field `slug`: `too_short`, `too_long`, `characters`, `hyphens`,
    /// `no_letter`, `looks_like_id`.
    pub fn parse(text: &str) -> Result<Self, ValidationErrors> {
        let mut problems = ValidationErrors::new();
        let mut problem = |code: &str, message: &str| {
            problems.add(ValidationError::new(Some("slug".to_owned()), code, message));
        };
        if text.len() < Self::MIN_LEN {
            problem("too_short", "at least 2 characters");
        }
        if text.len() > Self::MAX_LEN {
            problem("too_long", "at most 63 characters");
        }
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            problem("characters", "lowercase letters, digits and hyphens only");
        }
        if text.starts_with('-') || text.ends_with('-') || text.contains("--") {
            problem("hyphens", "single hyphens between letters or digits");
        }
        if !text.bytes().any(|byte| byte.is_ascii_lowercase()) {
            problem("no_letter", "at least one letter");
        }
        if looks_like_id(text) {
            problem("looks_like_id", "not shaped like an id");
        }
        match problems.is_empty() {
            true => Ok(Self(Cow::Owned(text.to_owned()))),
            false => Err(problems),
        }
    }

    /// Read a slug the way a person types one: any case, folded to lowercase.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse), for a text that is no slug in any case.
    pub fn read(text: &str) -> Result<Spelled<Self>, ValidationErrors> {
        let lowered = text.to_ascii_lowercase();
        let slug = Self::parse(&lowered)?;
        Ok(match lowered == text {
            true => Spelled::Canonical(slug),
            false => Spelled::Variant(slug),
        })
    }

    /// The slug as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The look-alike fold: hyphens dropped, `0` read as `o`, and `1`, `i` and `l` read as one
    /// symbol, so `paypa1` and `pay-pal` fold to `paypal`'s.
    #[must_use]
    pub fn fold(&self) -> String {
        self.0
            .chars()
            .filter(|c| *c != '-')
            .map(|c| match c {
                '0' => 'o',
                '1' | 'i' => 'l',
                other => other,
            })
            .collect()
    }
}

/// Whether `text` reads as a UUID (8-4-4-4-12 hex) or 32 hex digits.
fn looks_like_id(text: &str) -> bool {
    let hex = |part: &str| part.bytes().all(|byte| byte.is_ascii_hexdigit());
    let groups: Vec<&str> = text.split('-').collect();
    let uuid = groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|group| hex(group));
    uuid || (text.len() == 32 && hex(text))
}

/// Whether `bytes` spell a slug: the grammar [`parse`](Slug::parse) holds a text to, read while
/// the program compiles.
const fn is_slug(bytes: &[u8]) -> bool {
    if bytes.len() < Slug::MIN_LEN || bytes.len() > Slug::MAX_LEN {
        return false;
    }
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    let mut letter = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'-' && bytes[index - 1] == b'-' {
            return false;
        }
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return false;
        }
        letter |= byte.is_ascii_lowercase();
        index += 1;
    }
    letter && !is_id_shaped(bytes)
}

/// Whether `bytes` read as a UUID (8-4-4-4-12 hex) or 32 hex digits, for [`is_slug`].
const fn is_id_shaped(bytes: &[u8]) -> bool {
    let hyphens_at_uuid = bytes.len() == 36;
    let mut uuid = hyphens_at_uuid;
    let mut plain = bytes.len() == 32;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let hyphen_place = matches!(index, 8 | 13 | 18 | 23);
        uuid &= if hyphen_place {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        };
        plain &= byte.is_ascii_hexdigit();
        index += 1;
    }
    uuid || plain
}

impl fmt::Display for Slug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl PathSegment for Slug {
    fn to_segment(&self) -> String {
        self.0.clone().into_owned()
    }
}

impl fmt::Debug for Slug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Slug({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Spelled;

    use super::Slug;

    fn codes(text: &str) -> Vec<String> {
        Slug::parse(text)
            .err()
            .map(|problems| problems.errors().iter().map(|e| e.code.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_slug_reads() {
        assert_eq!(
            Slug::parse("acme-books")
                .map(|s| s.to_string())
                .ok()
                .as_deref(),
            Some("acme-books")
        );
        assert!(Slug::parse("a1").is_ok());
    }

    #[test]
    fn every_problem_is_collected() {
        assert_eq!(codes("-"), ["too_short", "hyphens", "no_letter"]);
        assert_eq!(codes("Acme--Books!"), ["characters", "hyphens"]);
        assert_eq!(codes("1234"), ["no_letter"]);
        assert_eq!(codes(&"a".repeat(64)), ["too_long"]);
    }

    #[test]
    fn a_slug_is_never_an_id() {
        assert_eq!(
            codes("769cf652-33df-4483-b200-ce0144d0aec7"),
            ["looks_like_id"]
        );
        assert_eq!(codes("769cf65233df4483b200ce0144d0aec7"), ["looks_like_id"]);
        assert!(codes("team_01h455vb4pex5vsknk084sn02q").contains(&"characters".to_owned()));
    }

    #[test]
    fn a_slug_written_in_the_source_is_checked_while_it_compiles() {
        const ACME: Slug = Slug::checked("acme");
        assert_eq!(Slug::parse("acme").ok(), Some(ACME));
    }

    #[test]
    fn the_check_while_compiling_takes_what_parse_takes() {
        for text in [
            "acme",
            "acme-books",
            "a1",
            "a",
            "-a",
            "a-",
            "a--b",
            "1234",
            "Ab",
            "a b",
            "a_b",
            "769cf652-33df-4483-b200-ce0144d0aec7",
            "769cf65233df4483b200ce0144d0aec7",
            "769cf652-33df-4483-b200-ce0144d0aec",
            &"a".repeat(63),
            &"a".repeat(64),
        ] {
            assert_eq!(
                super::is_slug(text.as_bytes()),
                Slug::parse(text).is_ok(),
                "{text}"
            );
        }
    }

    #[test]
    fn upper_case_reads_as_a_variant() {
        assert!(matches!(Slug::read("Acme-Books"), Ok(Spelled::Variant(_))));
        assert!(matches!(
            Slug::read("acme-books"),
            Ok(Spelled::Canonical(_))
        ));
    }

    #[test]
    fn look_alikes_fold_to_one_owner() {
        let paypal = Slug::parse("paypal").map(|s| s.fold()).ok();
        for alike in ["paypa1", "pay-pal", "p4ypal"] {
            let fold = Slug::parse(alike).map(|s| s.fold()).ok();
            assert_eq!(fold == paypal, alike != "p4ypal", "{alike}");
        }
        let admin = Slug::parse("admin").map(|s| s.fold()).ok();
        for alike in ["adm1n", "admln"] {
            assert_eq!(Slug::parse(alike).map(|s| s.fold()).ok(), admin, "{alike}");
        }
    }
}
