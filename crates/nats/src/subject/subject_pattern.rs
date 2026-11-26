use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::Subject;
use super::validation::validate_subject;
use crate::error::{NatsError, NatsResult};

/// A subject pattern for subscriptions, possibly containing the `*` (single
/// token) and `>` (one-or-more trailing tokens) wildcards.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubjectPattern(String);

impl SubjectPattern {
    /// Creates a pattern after validating it.
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`] if `pattern` is empty, has a leading,
    /// trailing, or consecutive dot, places `>` anywhere but last, or contains an
    /// invalid character.
    pub fn new(pattern: impl Into<String>) -> NatsResult<Self> {
        let pattern = pattern.into();
        validate_subject(&pattern, true)?;
        Ok(Self(pattern))
    }

    /// Creates a pattern without validating it.
    ///
    /// Only for a pattern already known to be valid.
    #[must_use]
    pub fn new_unchecked(pattern: impl Into<String>) -> Self {
        Self(pattern.into())
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether the pattern contains any wildcard.
    #[must_use]
    pub fn has_wildcards(&self) -> bool {
        self.0.contains('*') || self.0.contains('>')
    }

    /// Returns whether this pattern matches a concrete `subject`.
    #[must_use]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn matches(&self, subject: &Subject) -> bool {
        let pattern: Vec<&str> = self.0.split('.').collect();
        let tokens: Vec<&str> = subject.as_str().split('.').collect();

        let (mut p, mut t) = (0, 0);
        while p < pattern.len() && t < tokens.len() {
            match pattern[p] {
                ">" => return true,
                "*" => {
                    p += 1;
                    t += 1;
                }
                literal => {
                    if literal != tokens[t] {
                        return false;
                    }
                    p += 1;
                    t += 1;
                }
            }
        }
        p == pattern.len() && t == tokens.len()
    }

    /// Consumes the pattern, returning the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SubjectPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SubjectPattern {
    type Err = NatsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl AsRef<str> for SubjectPattern {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Subject> for SubjectPattern {
    fn from(subject: Subject) -> Self {
        Self(subject.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(s: &str) -> Subject {
        Subject::new(s).unwrap()
    }

    #[test]
    fn accepts_valid_patterns() {
        for ok in [
            "events.*",
            "events.>",
            "events.*.created",
            "events.user.created",
        ] {
            assert!(SubjectPattern::new(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_invalid_patterns() {
        assert!(SubjectPattern::new("").is_err());
        assert!(SubjectPattern::new("events.>.more").is_err());
    }

    #[test]
    fn single_token_wildcard() {
        let p = SubjectPattern::new("events.*").unwrap();
        assert!(p.matches(&subject("events.user")));
        assert!(p.matches(&subject("events.post")));
        assert!(!p.matches(&subject("events.user.created")));
        assert!(!p.matches(&subject("other.user")));
    }

    #[test]
    fn trailing_multi_wildcard() {
        let p = SubjectPattern::new("events.>").unwrap();
        assert!(p.matches(&subject("events.user")));
        assert!(p.matches(&subject("events.user.created")));
        assert!(p.matches(&subject("events.a.b.c.d")));
        assert!(!p.matches(&subject("other.user")));
    }

    #[test]
    fn mixed_wildcard_and_literal() {
        let p = SubjectPattern::new("events.*.created").unwrap();
        assert!(p.matches(&subject("events.user.created")));
        assert!(!p.matches(&subject("events.user.updated")));
        assert!(!p.matches(&subject("events.user.sub.created")));
    }

    #[test]
    fn has_wildcards_flag() {
        assert!(SubjectPattern::new("events.*").unwrap().has_wildcards());
        assert!(SubjectPattern::new("events.>").unwrap().has_wildcards());
        assert!(!SubjectPattern::new("events.user").unwrap().has_wildcards());
    }

    #[test]
    fn concrete_subject_becomes_pattern() {
        let p: SubjectPattern = subject("events.user").into();
        assert!(!p.has_wildcards());
        assert!(p.matches(&subject("events.user")));
    }
}
