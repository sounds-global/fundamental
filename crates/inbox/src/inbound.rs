//! [`Inbound`] — the event an effect is applying, as its handler sees it.

use fundamental_nats::{Delivery, Event, EventId, NatsResult, Subject};

/// An event delivered to an inbox handler: the decoded payload, its identity, and where it
/// came from.
#[derive(Debug)]
pub struct Inbound<M> {
    payload: M,
    event_id: EventId,
    subject: Subject,
    delivery: Delivery,
}

impl<M> Inbound<M> {
    pub(crate) fn new(payload: M, event_id: EventId, subject: Subject, delivery: Delivery) -> Self {
        Self {
            payload,
            event_id,
            subject,
            delivery,
        }
    }

    /// `payload` as an inbox handler receives it on `delivery`, for a test calling the handler.
    ///
    /// # Errors
    /// The subject's own error when the payload names no valid subject.
    pub fn delivered(payload: M, delivery: Delivery) -> NatsResult<Self>
    where
        M: Event,
    {
        let event_id = payload.event_id();
        let subject = Subject::new(payload.subject())?;
        Ok(Self::new(payload, event_id, subject, delivery))
    }

    /// The event.
    #[must_use]
    pub fn payload(&self) -> &M {
        &self.payload
    }

    /// Its identity — also the idempotency key for anything the effect sends elsewhere.
    #[must_use]
    pub fn event_id(&self) -> EventId {
        self.event_id
    }

    /// The subject it was published to.
    #[must_use]
    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    /// Where it sits in its stream, and which attempt this is.
    #[must_use]
    pub fn delivery(&self) -> &Delivery {
        &self.delivery
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_nats::{Event, Message};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct Booked {
        id: uuid::Uuid,
    }

    impl Message for Booked {
        const SUBJECT: &'static str = "billing.booked";
    }

    impl Event for Booked {
        fn event_id(&self) -> EventId {
            EventId::from_uuid(self.id)
        }
    }

    fn first() -> Delivery {
        Delivery {
            stream: "BILLING".to_owned(),
            stream_sequence: 1,
            consumer_sequence: 1,
            delivered: 1,
            published_at: chrono::DateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_test_delivers_an_event_as_its_handler_receives_it() {
        let id = uuid::Uuid::now_v7();
        let inbound = Inbound::delivered(Booked { id }, first()).expect("a valid subject");
        assert_eq!(inbound.event_id().as_uuid(), id);
        assert_eq!(inbound.subject().as_str(), "billing.booked");
        assert_eq!(inbound.delivery().delivered, 1);
    }
}
