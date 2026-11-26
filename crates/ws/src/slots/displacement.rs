//! [`Displacement`]: what asking for the oldest anonymous connection's slot did.

use tokio::time::Instant;

/// The outcome of [`AnonymousRegistry::displace_oldest`](crate::slots::AnonymousRegistry::displace_oldest).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Displacement {
    /// A connection past the floor was told to end; its permit follows once its socket closes.
    Displaced,
    /// Every anonymous connection is younger than the floor; the oldest crosses it at this instant.
    NotBefore(Instant),
    /// No anonymous connection is registered: the share is held by connections already ending.
    Empty,
}
