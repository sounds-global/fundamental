use std::borrow::Cow;

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::NatsResult;

/// A type that can be published to and received from NATS.
///
///
/// Implement it with `#[derive(Message)]`, with a static subject or a method computing one:
///
/// ```
/// # extern crate fundamental_nats as fundamental;
/// use fundamental::Message;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, Message)]
/// #[subject = "events.user.created"]
/// struct UserCreated {
///     user_id: String,
/// }
/// ```
///
/// The default wire form is JSON; override [`to_bytes`](Message::to_bytes) /
/// [`from_bytes`](Message::from_bytes) for a different codec.
pub trait Message: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The subject this message is published to; `"__dynamic__"` for a `#[subject_fn]` type, so
    /// read [`subject`](Message::subject) instead.
    const SUBJECT: &'static str;

    /// Returns the subject for this specific value. Defaults to [`SUBJECT`](Message::SUBJECT);
    /// dynamic-subject types override it.
    fn subject(&self) -> Cow<'static, str> {
        Cow::Borrowed(Self::SUBJECT)
    }

    /// Serializes the message to bytes (JSON by default).
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) if
    /// serialization fails.
    fn to_bytes(&self) -> NatsResult<Bytes> {
        Ok(Bytes::from(serde_json::to_vec(self)?))
    }

    /// Deserializes the message from bytes (JSON by default).
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) if
    /// deserialization fails.
    fn from_bytes(bytes: &[u8]) -> NatsResult<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Ping {
        seq: u64,
    }

    impl Message for Ping {
        const SUBJECT: &'static str = "ping";
    }

    #[test]
    fn default_subject_is_borrowed_const() {
        assert_eq!(Ping::SUBJECT, "ping");
        assert_eq!(Ping { seq: 1 }.subject(), Cow::Borrowed("ping"));
    }

    #[test]
    fn json_roundtrip() {
        let p = Ping { seq: 7 };
        let bytes = p.to_bytes().unwrap();
        assert_eq!(Ping::from_bytes(&bytes).unwrap(), p);
    }

    #[test]
    fn from_bytes_rejects_garbage() {
        assert!(Ping::from_bytes(b"not json").is_err());
    }
}
