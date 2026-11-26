use super::{EventId, Message};

/// A [`Message`] whose payload names the event it records, so a consumer can tell a second
/// delivery of it from a new event.
///
/// `#[derive(Message)]` implements it for a struct with one field marked `#[event_id]`, of
/// type [`EventId`].
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not name the event it records",
    label = "an inbox deduplicates on the identity the payload carries",
    note = "add an `EventId` field marked `#[event_id]` to `{Self}`, set with `EventId::generate()` where the event happens"
)]
pub trait Event: Message {
    /// This event's identity.
    fn event_id(&self) -> EventId;
}
