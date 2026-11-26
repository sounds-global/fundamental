use std::collections::HashMap;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Message;
use crate::error::NatsResult;

/// Wraps a payload with an id, type name, timestamp, and correlation and causation ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope<T> {
    /// Unique id for this message (UUID v4).
    pub id: String,
    /// The payload's Rust type name.
    #[serde(rename = "type")]
    pub message_type: String,
    /// The wrapped payload.
    pub payload: T,
    /// When the envelope was created.
    pub timestamp: DateTime<Utc>,
    /// The id tying this message to a wider conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// The id of the message that caused this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    /// Free-form metadata propagated with the message.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl<T> MessageEnvelope<T> {
    /// Wraps `payload`, stamping a fresh id, type name, and timestamp.
    #[must_use]
    pub fn new(payload: T) -> Self
    where
        T: Message,
    {
        Self {
            id: Uuid::new_v4().to_string(),
            message_type: std::any::type_name::<T>().to_string(),
            payload,
            timestamp: Utc::now(),
            correlation_id: None,
            causation_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Sets the correlation id.
    #[must_use]
    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Sets the causation id.
    #[must_use]
    pub fn with_causation(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }

    /// Adds a metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Maps the payload, preserving all envelope metadata.
    #[must_use]
    pub fn map<U, F>(self, f: F) -> MessageEnvelope<U>
    where
        F: FnOnce(T) -> U,
    {
        MessageEnvelope {
            id: self.id,
            message_type: self.message_type,
            payload: f(self.payload),
            timestamp: self.timestamp,
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
            metadata: self.metadata,
        }
    }
}

impl<T: Message> MessageEnvelope<T> {
    /// Serializes the envelope to bytes (JSON).
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) on failure.
    pub fn to_bytes(&self) -> NatsResult<Bytes> {
        Ok(Bytes::from(serde_json::to_vec(self)?))
    }

    /// Deserializes an envelope from bytes (JSON).
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) on failure.
    pub fn from_bytes(bytes: &[u8]) -> NatsResult<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Order {
        id: u64,
    }

    impl Message for Order {
        const SUBJECT: &'static str = "orders.created";
    }

    #[test]
    fn new_stamps_identity() {
        let env = MessageEnvelope::new(Order { id: 1 });
        assert!(!env.id.is_empty());
        assert!(env.message_type.contains("Order"));
        assert!(env.correlation_id.is_none());
    }

    #[test]
    fn builders_set_fields() {
        let env = MessageEnvelope::new(Order { id: 1 })
            .with_correlation("corr-1")
            .with_causation("cause-1")
            .with_metadata("tenant", "acme");
        assert_eq!(env.correlation_id.as_deref(), Some("corr-1"));
        assert_eq!(env.causation_id.as_deref(), Some("cause-1"));
        assert_eq!(env.metadata.get("tenant").map(String::as_str), Some("acme"));
    }

    #[test]
    fn map_preserves_metadata() {
        let env = MessageEnvelope::new(Order { id: 2 }).with_correlation("c");
        let mapped = env.map(|o| o.id);
        assert_eq!(mapped.payload, 2);
        assert_eq!(mapped.correlation_id.as_deref(), Some("c"));
    }

    #[test]
    fn bytes_roundtrip() {
        let env = MessageEnvelope::new(Order { id: 3 });
        let bytes = env.to_bytes().unwrap();
        let back: MessageEnvelope<Order> = MessageEnvelope::from_bytes(&bytes).unwrap();
        assert_eq!(back.payload, Order { id: 3 });
        assert_eq!(back.id, env.id);
    }
}
