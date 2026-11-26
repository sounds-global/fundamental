use fundamental_canonical::ContextDigest;

use super::EventPayload;

/// A BLAKE3 `derive_key` digest of an event's payload bytes: `PayloadDigest::derive(&[payload])`.
pub type PayloadDigest = ContextDigest<EventPayload>;
