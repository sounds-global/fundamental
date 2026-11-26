use crate::subject::Subject;

/// Dead-letter handling for a subscription: the sink subject and the retry
/// ceiling before a message is dead-lettered.
#[derive(Debug, Clone)]
pub struct DeadLetterConfig {
    /// The subject dead letters are published to.
    pub subject: Subject,
    /// The maximum delivery attempts before dead-lettering.
    pub max_retries: u32,
}

impl DeadLetterConfig {
    /// Creates a config targeting `subject` with the default retry ceiling of 3.
    #[must_use]
    pub fn new(subject: Subject) -> Self {
        Self {
            subject,
            max_retries: 3,
        }
    }

    /// Sets the maximum number of retries.
    #[must_use]
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_retries() {
        let cfg = DeadLetterConfig::new(Subject::new("dead.letter").unwrap()).with_max_retries(5);
        assert_eq!(cfg.subject.as_str(), "dead.letter");
        assert_eq!(cfg.max_retries, 5);
    }
}
