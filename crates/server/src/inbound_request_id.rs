/// Whether an inbound `X-Request-Id` is recorded as this request's id; a UUID can be a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InboundRequestId {
    /// Every request is recorded under an id this server mints. The default.
    #[default]
    Ignore,
    /// A caller's id is recorded when it parses as a [`RequestId`](crate::RequestId).
    Adopt,
}
