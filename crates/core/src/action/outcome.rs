//! What a write answers: what changed, the change's id, and the words that say so.

#[cfg(not(feature = "std"))]
use alloc::string::String;

use serde::{Deserialize, Serialize};

use super::ChangeId;

/// What a write answers: what changed, the id of that change, and the words that say so.
///
/// ```rust
/// use fundamental_core::action::{ChangeId, Outcome};
/// use uuid::Uuid;
///
/// let id = ChangeId::from_uuid(Uuid::nil());
/// let outcome = Outcome::changed(42).change(id).said("Saved.");
/// assert_eq!(outcome.changed, 42);
/// assert_eq!(outcome.change, Some(id));
/// assert_eq!(outcome.said.as_deref(), Some("Saved."));
///
/// let quiet = outcome.without_value();
/// assert_eq!(quiet.change, Some(id));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Outcome<T> {
    /// What the write changed: the record as it now stands, or `()`.
    pub changed: T,
    /// The id of the change, when the write recorded one an Undo can name.
    pub change: Option<ChangeId>,
    /// The words that tell the reader what happened, already in their language.
    pub said: Option<String>,
}

impl<T> Outcome<T> {
    /// An outcome that changed `value`, with no change id and no words yet.
    #[must_use]
    pub const fn changed(value: T) -> Self {
        Self {
            changed: value,
            change: None,
            said: None,
        }
    }

    /// Names the change this write recorded.
    #[must_use]
    pub fn change(mut self, id: ChangeId) -> Self {
        self.change = Some(id);
        self
    }

    /// Gives the words that tell the reader what happened.
    #[must_use]
    pub fn said(mut self, words: impl Into<String>) -> Self {
        self.said = Some(words.into());
        self
    }

    /// The same outcome without what changed: its change id and words only.
    #[must_use]
    pub fn without_value(self) -> Outcome<()> {
        Outcome {
            changed: (),
            change: self.change,
            said: self.said,
        }
    }
}

impl Outcome<()> {
    /// An outcome with nothing to show: no value, no change id, no words.
    #[must_use]
    pub const fn nothing() -> Self {
        Self::changed(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEXT: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    fn id() -> ChangeId {
        TEXT.parse().unwrap()
    }

    #[test]
    fn serializes_what_changed_its_change_and_its_words() {
        let outcome = Outcome::changed(7).change(id()).said("Saved.");
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({"changed": 7, "change": TEXT, "said": "Saved."})
        );
    }

    #[test]
    fn nothing_serializes_with_null_change_and_words() {
        assert_eq!(
            serde_json::to_value(Outcome::nothing()).unwrap(),
            json!({"changed": null, "change": null, "said": null})
        );
    }

    #[test]
    fn deserializes_without_change_or_words() {
        let outcome: Outcome<u8> = serde_json::from_value(json!({"changed": 3})).unwrap();
        assert_eq!(outcome, Outcome::changed(3));
    }

    #[test]
    fn without_value_keeps_the_change_and_the_words() {
        let outcome = Outcome::changed("record").change(id()).said("Saved.");
        assert_eq!(
            outcome.without_value(),
            Outcome::nothing().change(id()).said("Saved.")
        );
    }

    #[test]
    fn refuses_a_change_that_is_not_a_uuid() {
        let refused =
            serde_json::from_value::<Outcome<()>>(json!({"changed": null, "change": "nope"}));
        assert!(refused.is_err());
    }
}
