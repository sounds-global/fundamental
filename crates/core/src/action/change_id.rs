//! The id of one change a write made.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The id of one change a write made: what an Undo names and a history entry is keyed by.
///
/// ```rust
/// use fundamental_core::action::ChangeId;
/// use uuid::Uuid;
///
/// let id = ChangeId::from_uuid(Uuid::nil());
/// let parsed: ChangeId = id.to_string().parse().unwrap();
/// assert_eq!(parsed, id);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChangeId(Uuid);

impl ChangeId {
    /// Wraps the UUID a change was recorded under.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the UUID this change was recorded under.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for ChangeId {
    type Err = uuid::Error;

    /// Parses a change id from its UUID text.
    ///
    /// # Errors
    ///
    /// Returns the [`uuid::Error`] when the text is not a UUID.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(text).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    #[test]
    fn serializes_as_the_bare_uuid_string() {
        let id: ChangeId = TEXT.parse().unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{TEXT}\""));
    }

    #[test]
    fn deserializes_from_the_uuid_string() {
        let id: ChangeId = serde_json::from_str(&format!("\"{TEXT}\"")).unwrap();
        assert_eq!(id.to_string(), TEXT);
    }

    #[test]
    fn keeps_the_uuid_it_wraps() {
        let uuid = Uuid::parse_str(TEXT).unwrap();
        assert_eq!(ChangeId::from_uuid(uuid).as_uuid(), uuid);
    }

    #[test]
    fn refuses_text_that_is_not_a_uuid() {
        assert!("not-a-change".parse::<ChangeId>().is_err());
    }
}
