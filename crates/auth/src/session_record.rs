//! [`SessionRecord`] — what a store says a live session is.

use fundamental_core::{DeviceId, UserId};

/// A live session as its store holds it, keyed by its handle's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// Whose session it is.
    pub subject: UserId,
    /// The device the session was opened on.
    pub device: Option<DeviceId>,
    /// The scopes the session grants.
    pub scopes: Vec<String>,
    /// When the session ends, in seconds since the Unix epoch; enforced even before the row is removed.
    pub not_after: i64,
}
