use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A message diverted to a dead-letter sink, with enough context to diagnose and replay it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    /// The original message id.
    pub message_id: String,
    /// The subject the message was received on.
    pub subject: String,
    /// The original payload, as a string (typically JSON).
    pub payload: String,
    /// Why the message was dead-lettered.
    pub reason: DeadLetterReason,
    /// The error detail, if any.
    pub error: Option<String>,
    /// How many delivery attempts were made.
    pub delivery_count: u32,
    /// When the original message was produced.
    pub original_timestamp: DateTime<Utc>,
    /// When the message was dead-lettered.
    pub dead_lettered_at: DateTime<Utc>,
    /// The original message headers.
    pub headers: HashMap<String, String>,
}

impl DeadLetter {
    /// Creates a dead letter from the original message id, subject, payload and reason.
    pub fn new(
        message_id: impl Into<String>,
        subject: impl Into<String>,
        payload: impl Into<String>,
        reason: DeadLetterReason,
    ) -> Self {
        let now = Utc::now();
        Self {
            message_id: message_id.into(),
            subject: subject.into(),
            payload: payload.into(),
            reason,
            error: None,
            delivery_count: 0,
            original_timestamp: now,
            dead_lettered_at: now,
            headers: HashMap::new(),
        }
    }

    /// Attaches an error detail.
    #[must_use]
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sets the delivery-attempt count.
    #[must_use]
    pub fn with_delivery_count(mut self, count: u32) -> Self {
        self.delivery_count = count;
        self
    }
}

/// Why a message was dead-lettered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeadLetterReason {
    /// The payload could not be deserialized.
    DeserializationFailed,
    /// The handler returned an error.
    ProcessingFailed,
    /// Delivery exceeded the retry limit.
    MaxRetriesExceeded,
    /// The message expired before processing.
    Expired,
    /// The message was explicitly rejected.
    Rejected,
    /// The reason is unknown.
    Unknown,
}

impl fmt::Display for DeadLetterReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::DeserializationFailed => "deserialization_failed",
            Self::ProcessingFailed => "processing_failed",
            Self::MaxRetriesExceeded => "max_retries_exceeded",
            Self::Expired => "expired",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_fields() {
        let dl = DeadLetter::new(
            "id-1",
            "events.user",
            "{}",
            DeadLetterReason::ProcessingFailed,
        )
        .with_error("boom")
        .with_delivery_count(3);
        assert_eq!(dl.message_id, "id-1");
        assert_eq!(dl.reason, DeadLetterReason::ProcessingFailed);
        assert_eq!(dl.error.as_deref(), Some("boom"));
        assert_eq!(dl.delivery_count, 3);
    }

    #[test]
    fn reason_serializes_snake_case() {
        let json = serde_json::to_string(&DeadLetterReason::MaxRetriesExceeded).unwrap();
        assert_eq!(json, "\"max_retries_exceeded\"");
        assert_eq!(
            DeadLetterReason::MaxRetriesExceeded.to_string(),
            "max_retries_exceeded"
        );
    }
}
