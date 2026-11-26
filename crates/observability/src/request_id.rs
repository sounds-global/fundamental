use std::fmt;

use uuid::Uuid;

use crate::RequestIdRejection;

/// The only form accepted.
const HYPHENATED_LEN: usize = 36;

/// From this length a value is classed oversized rather than malformed.
const OVERSIZED_FROM: usize = 64;

/// The id every log line and span of one request carries.
///
/// A UUID only, so it holds nothing a caller chose beyond its 128 bits; minted from the OS
/// CSPRNG or parsed from the hyphenated form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(Uuid);

impl RequestId {
    /// A fresh random (version 4) id.
    #[must_use]
    pub fn mint() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses the 36-character hyphenated form, in either case.
    ///
    /// Other spellings, and the nil and max UUIDs, are refused.
    /// # Errors
    ///
    /// A [`RequestIdRejection`] naming the class of the failure — never the value.
    pub fn parse(value: &str) -> Result<Self, RequestIdRejection> {
        if value.len() >= OVERSIZED_FROM {
            return Err(RequestIdRejection::Oversized);
        }
        if value.len() != HYPHENATED_LEN {
            return Err(RequestIdRejection::Malformed);
        }
        let uuid = Uuid::try_parse(value).map_err(|_| RequestIdRejection::Malformed)?;
        if uuid.is_nil() || uuid.is_max() {
            return Err(RequestIdRejection::Reserved);
        }
        Ok(Self(uuid))
    }
}

impl fmt::Display for RequestId {
    /// The lower-case hyphenated form, whatever case it was read in.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_id_parses_back_to_itself() {
        let minted = RequestId::mint();
        assert_eq!(RequestId::parse(&minted.to_string()), Ok(minted));
    }

    #[test]
    fn two_minted_ids_differ() {
        assert_ne!(RequestId::mint(), RequestId::mint());
    }

    #[test]
    fn an_upper_case_id_is_rendered_lower_case() {
        let parsed = RequestId::parse("67E55044-10B1-426F-9247-BB680E5FE0C8").expect("valid");
        assert_eq!(parsed.to_string(), "67e55044-10b1-426f-9247-bb680e5fe0c8");
    }

    #[test]
    fn other_uuid_spellings_are_malformed() {
        for other in [
            "67e5504410b1426f9247bb680e5fe0c8",
            "{67e55044-10b1-426f-9247-bb680e5fe0c8}",
            "urn:uuid:67e55044-10b1-426f-9247-bb680e5fe0c8",
        ] {
            assert_eq!(
                RequestId::parse(other),
                Err(RequestIdRejection::Malformed),
                "{other}"
            );
        }
    }

    #[test]
    fn hostile_values_are_refused_by_class() {
        let cases = [
            ("jan@example.nl", RequestIdRejection::Malformed),
            ("", RequestIdRejection::Malformed),
            (
                "67e55044-10b1-426f-9247-bb680e5fe0c\n",
                RequestIdRejection::Malformed,
            ),
            (
                "67e55044-10b1-426f-9247-bb680e5fe0cé",
                RequestIdRejection::Malformed,
            ),
            (&"a".repeat(64), RequestIdRejection::Oversized),
            (&"x".repeat(10_000), RequestIdRejection::Oversized),
        ];
        for (value, expected) in cases {
            assert_eq!(RequestId::parse(value), Err(expected), "{value:?}");
        }
    }

    #[test]
    fn the_nil_and_max_ids_are_reserved() {
        for reserved in [
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            assert_eq!(
                RequestId::parse(reserved),
                Err(RequestIdRejection::Reserved)
            );
        }
    }
}
