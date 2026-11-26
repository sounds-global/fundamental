use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::validation::{validate_subject, validate_token};
use crate::error::{NatsError, NatsResult};

/// A validated NATS subject: a dot-separated hierarchical name with no
/// wildcards, suitable for publishing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Subject(String);

impl Subject {
    /// Creates a subject after validating it.
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`] if `subject` is empty, has a leading,
    /// trailing, or consecutive dot, contains a wildcard, or contains a character
    /// outside `[A-Za-z0-9._-]`.
    pub fn new(subject: impl Into<String>) -> NatsResult<Self> {
        let subject = subject.into();
        validate_subject(&subject, false)?;
        Ok(Self(subject))
    }

    /// Creates a subject without validating it.
    ///
    ///
    /// Only for a subject already known to be valid.
    #[must_use]
    pub fn new_unchecked(subject: impl Into<String>) -> Self {
        Self(subject.into())
    }

    /// Returns the subject as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the dot-separated tokens of the subject.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.0.split('.')
    }

    /// Returns the number of tokens in the subject.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.0.split('.').count()
    }

    /// Returns the parent subject (everything before the last token), or `None`
    /// for a single-token subject.
    #[must_use]
    pub fn parent(&self) -> Option<Subject> {
        let idx = self.0.rfind('.')?;
        Some(Subject(self.0[..idx].to_string()))
    }

    /// Returns a new subject with `token` appended.
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`] if `token` is empty or contains an
    /// invalid character.
    pub fn push(&self, token: &str) -> NatsResult<Subject> {
        validate_token(token)?;
        Ok(Subject(format!("{}.{}", self.0, token)))
    }

    /// Consumes the subject, returning the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Subject {
    type Err = NatsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl AsRef<str> for Subject {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Subject> for String {
    fn from(subject: Subject) -> Self {
        subject.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_subjects() {
        assert!(Subject::new("events.user.created").is_ok());
        assert!(Subject::new("simple").is_ok());
        assert!(Subject::new("a.b.c.d.e").is_ok());
        assert!(Subject::new("tenant-123.events_v2").is_ok());
    }

    #[test]
    fn rejects_invalid_subjects() {
        for bad in [
            "",
            ".leading",
            "trailing.",
            "double..dot",
            "with space",
            "events.*",
        ] {
            let err = Subject::new(bad).unwrap_err();
            assert!(matches!(err, NatsError::InvalidSubject { .. }), "{bad}");
        }
    }

    #[test]
    fn tokens_and_count() {
        let s = Subject::new("events.user.created").unwrap();
        assert_eq!(
            s.tokens().collect::<Vec<_>>(),
            ["events", "user", "created"]
        );
        assert_eq!(s.token_count(), 3);
    }

    #[test]
    fn parent_of_multi_and_single_token() {
        let s = Subject::new("events.user.created").unwrap();
        assert_eq!(s.parent().unwrap().as_str(), "events.user");
        assert!(Subject::new("events").unwrap().parent().is_none());
    }

    #[test]
    fn push_appends_or_rejects() {
        let s = Subject::new("events").unwrap();
        assert_eq!(s.push("user").unwrap().as_str(), "events.user");
        assert!(s.push("bad token").is_err());
    }

    #[test]
    fn roundtrips_through_str() {
        let s: Subject = "events.user".parse().unwrap();
        assert_eq!(s.to_string(), "events.user");
        assert!("events.*".parse::<Subject>().is_err());
    }
}
