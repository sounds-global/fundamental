use super::DeadLetterConfig;

/// Options for a subscription.
#[derive(Debug, Clone)]
pub struct SubscriptionOptions {
    /// Queue group for load-balanced delivery across subscribers.
    pub queue_group: Option<String>,
    /// Whether to skip and log undeserializable messages (default) rather than
    /// close the subscription.
    pub skip_invalid: bool,
    /// Per-subscription buffer capacity.
    pub buffer_size: usize,
    /// Dead-letter handling.
    pub dead_letter: Option<DeadLetterConfig>,
}

impl Default for SubscriptionOptions {
    fn default() -> Self {
        Self {
            queue_group: None,
            skip_invalid: true,
            buffer_size: 4096,
            dead_letter: None,
        }
    }
}

impl SubscriptionOptions {
    /// Creates default options (skip invalid messages, no queue group).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the queue group, distributing messages across same-group subscribers.
    #[must_use]
    pub fn with_queue_group(mut self, group: impl Into<String>) -> Self {
        self.queue_group = Some(group.into());
        self
    }

    /// Sets whether undeserializable messages are skipped (`true`) or close the
    /// subscription (`false`).
    #[must_use]
    pub fn skip_invalid(mut self, skip: bool) -> Self {
        self.skip_invalid = skip;
        self
    }

    /// Sets the buffer capacity.
    #[must_use]
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Enables dead-letter handling.
    #[must_use]
    pub fn with_dead_letter(mut self, config: DeadLetterConfig) -> Self {
        self.dead_letter = Some(config);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_skip_invalid() {
        let opts = SubscriptionOptions::new();
        assert!(opts.skip_invalid);
        assert!(opts.queue_group.is_none());
        assert_eq!(opts.buffer_size, 4096);
    }

    #[test]
    fn builders_set_fields() {
        let opts = SubscriptionOptions::new()
            .with_queue_group("workers")
            .with_buffer_size(1000)
            .skip_invalid(false);
        assert_eq!(opts.queue_group.as_deref(), Some("workers"));
        assert_eq!(opts.buffer_size, 1000);
        assert!(!opts.skip_invalid);
    }
}
