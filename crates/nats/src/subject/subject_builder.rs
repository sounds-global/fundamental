use super::{Subject, SubjectPattern};
use crate::error::{NatsError, NatsResult};

/// Builds a [`Subject`] or [`SubjectPattern`] token by token.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct SubjectBuilder {
    tokens: Vec<String>,
}

impl SubjectBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a token.
    pub fn push(mut self, token: impl Into<String>) -> Self {
        self.tokens.push(token.into());
        self
    }

    /// Builds a concrete [`Subject`].
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`] if no tokens were pushed or the
    /// joined subject fails validation.
    pub fn build(self) -> NatsResult<Subject> {
        if self.tokens.is_empty() {
            return Err(NatsError::invalid_subject("", "subject cannot be empty"));
        }
        Subject::new(self.tokens.join("."))
    }

    /// Builds a [`SubjectPattern`].
    ///
    /// # Errors
    /// Returns [`NatsError::InvalidSubject`] if no tokens were pushed or the
    /// joined pattern fails validation.
    pub fn build_pattern(self) -> NatsResult<SubjectPattern> {
        if self.tokens.is_empty() {
            return Err(NatsError::invalid_subject("", "pattern cannot be empty"));
        }
        SubjectPattern::new(self.tokens.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_subject() {
        let s = SubjectBuilder::new()
            .push("events")
            .push("user")
            .push("created")
            .build()
            .unwrap();
        assert_eq!(s.as_str(), "events.user.created");
    }

    #[test]
    fn builds_pattern() {
        let p = SubjectBuilder::new()
            .push("events")
            .push("*")
            .build_pattern()
            .unwrap();
        assert_eq!(p.as_str(), "events.*");
    }

    #[test]
    fn empty_builder_errors() {
        assert!(SubjectBuilder::new().build().is_err());
        assert!(SubjectBuilder::new().build_pattern().is_err());
    }
}
