use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::PayloadDigest;

/// The identity of one event: a UUIDv7 generated where the event happens, carried in its payload.
///
/// In the payload, so it survives a hop that drops headers and is covered by the digest. Decoding
/// accepts any UUID; [`is_v7`](Self::is_v7) is the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventId(Uuid);

impl EventId {
    /// A new identity, for an event happening now.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Wraps a UUID that already names an event — an outbox row's id, say.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// The UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Whether this is a UUIDv7, the only version that says when its event occurred.
    #[must_use]
    pub fn is_v7(&self) -> bool {
        self.0.get_version_num() == 7
    }

    /// When the event occurred, from a UUIDv7's timestamp; `None` for any other version.
    #[must_use]
    pub fn occurred_at(&self) -> Option<DateTime<Utc>> {
        if !self.is_v7() {
            return None;
        }
        let (seconds, nanos) = self.0.get_timestamp()?.to_unix();
        DateTime::from_timestamp(i64::try_from(seconds).ok()?, nanos)
    }

    /// A number in `0..64` spread evenly across events, from the UUID's random bits.
    #[must_use]
    pub fn bucket(&self) -> u8 {
        self.0.as_bytes()[15] & 0x3f
    }
}

impl EventId {
    /// The `Nats-Msg-Id` a message carrying this event as `payload` is published under:
    /// `<event id>.<digest>`.
    ///
    /// With the digest, broker de-duplication merges only byte-identical republishes, so a reused
    /// id reaches the consumer.
    #[must_use]
    pub fn msg_id_for(&self, payload: &[u8]) -> String {
        format!(
            "{self}.{}",
            PayloadDigest::derive(&[payload]).to_base64url()
        )
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_id_is_a_v7_that_says_when() {
        let before = Utc::now() - chrono::Duration::milliseconds(1);
        let id = EventId::generate();
        let after = Utc::now() + chrono::Duration::milliseconds(1);
        assert!(id.is_v7());
        let occurred = id.occurred_at().expect("a v7 has a timestamp");
        assert!(
            before <= occurred && occurred <= after,
            "{occurred} not in [{before}, {after}]"
        );
    }

    #[test]
    fn a_v4_is_not_a_v7_and_has_no_time() {
        let id = EventId::from_uuid(Uuid::new_v4());
        assert!(!id.is_v7());
        assert_eq!(id.occurred_at(), None);
    }

    #[test]
    fn the_bucket_is_the_low_six_bits_of_the_last_byte() {
        let id = EventId::from_uuid(Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00ff));
        assert_eq!(id.bucket(), 0x3f);
        let id = EventId::from_uuid(Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0040));
        assert_eq!(id.bucket(), 0);
    }

    #[test]
    fn the_message_id_names_the_event_and_what_it_said() {
        let id = EventId::generate();
        let paid = id.msg_id_for(b"{\"cents\":1}");
        assert!(paid.starts_with(&format!("{id}.")), "{paid}");
        assert_eq!(paid, id.msg_id_for(b"{\"cents\":1}"));
        assert_ne!(
            paid,
            id.msg_id_for(b"{\"cents\":2}"),
            "a reused id must be a new message id"
        );
    }

    #[test]
    fn it_travels_as_the_hyphenated_uuid() {
        let uuid = Uuid::now_v7();
        let id = EventId::from_uuid(uuid);
        assert_eq!(
            serde_json::to_string(&id).expect("json"),
            format!("\"{uuid}\"")
        );
        assert_eq!(id.to_string(), uuid.to_string());
    }
}
