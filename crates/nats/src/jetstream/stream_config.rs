use std::time::Duration;

use async_nats::jetstream;
use serde::{Deserialize, Serialize};

use super::stream_drift::limit;
use super::{DiscardPolicy, RetentionPolicy, StorageType};

/// Configuration for creating or updating a JetStream stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// The stream name.
    pub name: String,
    /// Subjects the stream captures.
    #[serde(default)]
    pub subjects: Vec<String>,
    /// A human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Maximum number of messages to retain.
    #[serde(default)]
    pub max_messages: Option<i64>,
    /// Maximum total bytes to retain.
    #[serde(default)]
    pub max_bytes: Option<i64>,
    /// Maximum age of a message.
    #[serde(default)]
    pub max_age: Option<Duration>,
    /// Maximum size of a single message.
    #[serde(default)]
    pub max_message_size: Option<i32>,
    /// Storage backend.
    #[serde(default)]
    pub storage: StorageType,
    /// Replica count.
    #[serde(default = "default_replicas")]
    pub replicas: usize,
    /// Retention policy.
    #[serde(default)]
    pub retention: RetentionPolicy,
    /// Discard policy at the limit.
    #[serde(default)]
    pub discard: DiscardPolicy,
    /// How long a repeated `Nats-Msg-Id` is a duplicate; the server's default when unset.
    #[serde(default)]
    pub duplicate_window: Option<Duration>,
    /// Refuse deleting an individual message.
    #[serde(default)]
    pub deny_delete: bool,
    /// Refuse purging the stream.
    #[serde(default)]
    pub deny_purge: bool,
}

fn default_replicas() -> usize {
    1
}

impl StreamConfig {
    /// Creates a stream config named `name` with default policies.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            subjects: Vec::new(),
            description: None,
            max_messages: None,
            max_bytes: None,
            max_age: None,
            max_message_size: None,
            storage: StorageType::File,
            replicas: default_replicas(),
            retention: RetentionPolicy::Limits,
            discard: DiscardPolicy::Old,
            duplicate_window: None,
            deny_delete: false,
            deny_purge: false,
        }
    }

    /// Sets the captured subjects.
    #[must_use]
    pub fn subjects<I, S>(mut self, subjects: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.subjects = subjects.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Sets the maximum message count.
    #[must_use]
    pub fn max_messages(mut self, max: i64) -> Self {
        self.max_messages = Some(max);
        self
    }

    /// Sets the maximum total bytes.
    #[must_use]
    pub fn max_bytes(mut self, max: i64) -> Self {
        self.max_bytes = Some(max);
        self
    }

    /// Sets the maximum message age.
    #[must_use]
    pub fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// Sets the storage backend.
    #[must_use]
    pub fn storage(mut self, storage: StorageType) -> Self {
        self.storage = storage;
        self
    }

    /// Sets the replica count.
    #[must_use]
    pub fn replicas(mut self, replicas: usize) -> Self {
        self.replicas = replicas;
        self
    }

    /// Sets the retention policy.
    #[must_use]
    pub fn retention(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// Sets what happens to a publish once the stream is full.
    #[must_use]
    pub fn discard(mut self, discard: DiscardPolicy) -> Self {
        self.discard = discard;
        self
    }

    /// Sets the largest single message the stream accepts.
    #[must_use]
    pub fn max_message_size(mut self, bytes: i32) -> Self {
        self.max_message_size = Some(bytes);
        self
    }

    /// Sets how long a repeated `Nats-Msg-Id` is recognised as a duplicate.
    #[must_use]
    pub fn duplicate_window(mut self, window: Duration) -> Self {
        self.duplicate_window = Some(window);
        self
    }

    /// Refuses (or allows) deleting an individual message.
    #[must_use]
    pub fn deny_delete(mut self, deny: bool) -> Self {
        self.deny_delete = deny;
        self
    }

    /// Refuses (or allows) purging the stream.
    #[must_use]
    pub fn deny_purge(mut self, deny: bool) -> Self {
        self.deny_purge = deny;
        self
    }

    /// The configuration a server reports, in this type's terms: unlimited is `None` and
    /// subjects lose `prefix`.
    pub(crate) fn from_jetstream(
        config: &jetstream::stream::Config,
        prefix: Option<&crate::subject::SubjectPrefix>,
    ) -> Self {
        Self {
            name: config.name.clone(),
            subjects: config
                .subjects
                .iter()
                .map(|subject| {
                    prefix
                        .map_or(subject.as_str(), |p| p.strip(subject))
                        .to_owned()
                })
                .collect(),
            description: config.description.clone(),
            max_messages: limit(config.max_messages),
            max_bytes: limit(config.max_bytes),
            max_age: (!config.max_age.is_zero()).then_some(config.max_age),
            max_message_size: limit(config.max_message_size),
            storage: config.storage.into(),
            replicas: config.num_replicas,
            retention: config.retention.into(),
            discard: config.discard.into(),
            duplicate_window: (!config.duplicate_window.is_zero())
                .then_some(config.duplicate_window),
            deny_delete: config.deny_delete,
            deny_purge: config.deny_purge,
        }
    }

    pub(crate) fn into_jetstream_config(self) -> jetstream::stream::Config {
        let mut config = jetstream::stream::Config {
            name: self.name,
            subjects: self.subjects,
            description: self.description,
            num_replicas: self.replicas,
            storage: self.storage.into(),
            retention: self.retention.into(),
            discard: self.discard.into(),
            deny_delete: self.deny_delete,
            deny_purge: self.deny_purge,
            ..Default::default()
        };
        if let Some(window) = self.duplicate_window {
            config.duplicate_window = window;
        }
        if let Some(max) = self.max_messages {
            config.max_messages = max;
        }
        if let Some(max) = self.max_bytes {
            config.max_bytes = max;
        }
        if let Some(age) = self.max_age {
            config.max_age = age;
        }
        if let Some(size) = self.max_message_size {
            config.max_message_size = size;
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_fields() {
        let cfg = StreamConfig::new("ORDERS")
            .subjects(["orders.>"])
            .max_messages(1_000_000)
            .storage(StorageType::File)
            .retention(RetentionPolicy::Limits);
        assert_eq!(cfg.name, "ORDERS");
        assert_eq!(cfg.subjects, ["orders.>"]);
        assert_eq!(cfg.max_messages, Some(1_000_000));
        assert_eq!(cfg.storage, StorageType::File);
    }
}
