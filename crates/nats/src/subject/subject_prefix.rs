use core::fmt;

use serde::{Deserialize, Serialize};

use super::validation::validate_subject;
use crate::error::NatsResult;

/// A namespace prepended to every subject a [`NatsClient`](crate::NatsClient) names.
///
/// The counterpart of [`NatsConfig::inbox_prefix`](crate::NatsConfig) for publishes and
/// subscriptions: one cluster for many environments, or test runs that cannot collide. A
/// namespace, not a security boundary. A valid wildcard-free subject, joined with a dot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubjectPrefix(String);

impl SubjectPrefix {
    /// Creates a prefix after validating it.
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`](crate::NatsError) if `prefix` is empty, has a
    /// leading, trailing or consecutive dot, contains a wildcard, or contains a character
    /// outside `[A-Za-z0-9._-]`.
    pub fn new(prefix: impl Into<String>) -> NatsResult<Self> {
        let prefix = prefix.into();
        validate_subject(&prefix, false)?;
        Ok(Self(prefix))
    }

    /// The prefix, as it appears at the front of a subject.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `subject`, as this connection names it on the wire.
    #[must_use]
    pub fn apply(&self, subject: &str) -> String {
        format!("{}.{subject}", self.0)
    }

    /// `subject` as the caller wrote it, with this prefix removed.
    ///
    /// Inbound, so `MessageSet::from_parts` matches the subject it was compiled with. A subject
    /// without this prefix, such as an `_INBOX` reply, is returned untouched.
    #[must_use]
    pub fn strip<'a>(&self, subject: &'a str) -> &'a str {
        subject
            .strip_prefix(self.0.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(subject)
    }
}

impl fmt::Display for SubjectPrefix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_and_a_subject_join_with_one_dot() {
        let prefix = SubjectPrefix::new("eu.prod").expect("a valid prefix");
        assert_eq!(prefix.apply("orders.created"), "eu.prod.orders.created");
    }

    #[test]
    fn stripping_undoes_applying() {
        let prefix = SubjectPrefix::new("run-7f3a").expect("a valid prefix");
        let subject = "set.events.created";
        assert_eq!(prefix.strip(&prefix.apply(subject)), subject);
    }

    #[test]
    fn a_subject_that_does_not_carry_the_prefix_is_left_alone() {
        // A reply on `_INBOX.…` is not trimmed.
        let prefix = SubjectPrefix::new("eu.prod").expect("a valid prefix");
        assert_eq!(prefix.strip("_INBOX.abc.1"), "_INBOX.abc.1");
        assert_eq!(prefix.strip("orders.created"), "orders.created");
    }

    #[test]
    fn a_prefix_that_is_only_a_partial_token_does_not_match() {
        // The dot is part of the boundary.
        let prefix = SubjectPrefix::new("eu").expect("a valid prefix");
        assert_eq!(prefix.strip("europe.orders"), "europe.orders");
        assert_eq!(prefix.strip("eu.orders"), "orders");
    }

    #[test]
    fn a_wildcard_prefix_is_refused() {
        // A wildcard would subscribe to a whole subject tree.
        for refused in [">", "eu.*", "eu.>", "", ".eu", "eu.", "eu..prod", "eu prod"] {
            assert!(
                SubjectPrefix::new(refused).is_err(),
                "{refused:?} must not be accepted as a prefix"
            );
        }
    }
}
