use std::fmt;

/// A subscription this process declared: the subject and queue group it asked for.
///
/// Declared, not established: `async_nats::subscribe` returns `Ok` without a server answer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclaredSubscription {
    /// The subject pattern, as sent.
    pub subject: String,
    /// The queue group, when the subscription joined one.
    pub queue_group: Option<String>,
}

impl fmt::Display for DeclaredSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.queue_group {
            Some(group) => write!(f, "{} (queue {group})", self.subject),
            None => f.write_str(&self.subject),
        }
    }
}
