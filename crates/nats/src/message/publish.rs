use std::borrow::Cow;

use bytes::Bytes;

use crate::error::NatsResult;

/// A value that can be published to NATS: its subject and its wire bytes.
///
/// Every [`Message`](crate::Message) and [`MessageSet`](crate::MessageSet) is `Publish`.
pub trait Publish: Send + Sync + 'static {
    /// The subject this value is published to.
    fn subject(&self) -> Cow<'static, str>;

    /// Serializes the value to its wire bytes (JSON by default).
    ///
    /// # Errors
    /// Returns [`NatsError::Serialization`](crate::NatsError::Serialization) if
    /// serialization fails.
    fn to_bytes(&self) -> NatsResult<Bytes>;
}
