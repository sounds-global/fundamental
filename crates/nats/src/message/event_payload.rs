use fundamental_canonical::DigestContext;

/// The digest context of an event's payload bytes, telling a redelivery from a reused id.
#[derive(Debug, Clone, Copy)]
pub struct EventPayload;

impl DigestContext for EventPayload {
    const CONTEXT: &'static str = "fundamental nats 2026-09-30 event payload digest v1";
}
