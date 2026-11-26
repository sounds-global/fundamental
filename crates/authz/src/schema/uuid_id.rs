//! [`uuid_id`]: the canonical id of a namespace whose ids are UUIDs.

use std::borrow::Cow;

/// Reduces any spelling of a UUID — upper-case, simple, braced, a URN — to its one spelling,
/// lower-case and hyphenated; anything else is `None`, which denies.
///
/// Set it with `#[namespace(canonical_id = fundamental::authz::schema::uuid_id)]`, so `7A3…`
/// and `7a3…` are one object.
#[must_use]
pub fn uuid_id(raw: &str) -> Option<Cow<'_, str>> {
    let parsed = uuid::Uuid::try_parse(raw).ok()?;
    let canonical = parsed.hyphenated().to_string();
    Some(match canonical == raw {
        true => Cow::Borrowed(raw),
        false => Cow::Owned(canonical),
    })
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::uuid_id;

    const ID: &str = "769cf652-33df-4483-b200-ce0144d0aec7";

    #[test]
    fn every_spelling_of_a_uuid_reduces_to_one() {
        assert_eq!(uuid_id(ID), Some(Cow::Borrowed(ID)));
        for spelling in [
            "769CF652-33DF-4483-B200-CE0144D0AEC7",
            "769cf65233df4483b200ce0144d0aec7",
            "{769cf652-33df-4483-b200-ce0144d0aec7}",
            "urn:uuid:769cf652-33df-4483-b200-ce0144d0aec7",
        ] {
            assert_eq!(uuid_id(spelling).as_deref(), Some(ID), "{spelling}");
        }
    }

    #[test]
    fn a_public_spelling_is_not_an_id_and_denies() {
        for refused in ["acme-corp", "team_01h455vb4pex5vsknk084sn02q", "", "12"] {
            assert_eq!(uuid_id(refused), None, "{refused}");
        }
    }
}
