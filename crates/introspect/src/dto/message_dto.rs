use serde::{Deserialize, Serialize};

use crate::dto::schema_dto::SchemaDto;

/// One `#[message]`/`#[respond]` handler as fetched surface data; mirrors [`MessageDescriptor`](fundamental_nats::MessageDescriptor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageDto {
    /// The subject the message is published to (`"__dynamic__"` for a per-value subject).
    pub subject: String,
    /// The display label for the transport (`"nats"`, `"jetstream"`, `"socket"`, `"nats_rpc"`).
    pub transport: String,
    /// The message's documentation (empty when undocumented).
    pub doc: String,
    /// The payload's shape (the *request* payload for an RPC).
    pub payload: SchemaDto,
    /// The reply payload's shape for a request/reply RPC; `None` for one-way pub/sub.
    #[serde(default)]
    pub reply: Option<SchemaDto>,
}

#[cfg(feature = "ssr")]
impl From<&fundamental_nats::MessageDescriptor> for MessageDto {
    fn from(message: &fundamental_nats::MessageDescriptor) -> Self {
        Self {
            subject: message.subject.to_owned(),
            transport: message.transport_label().to_owned(),
            doc: message.doc.to_owned(),
            payload: SchemaDto::from(&message.payload),
            reply: message.reply.as_ref().map(SchemaDto::from),
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use fundamental_nats::{MessageDescriptor, MessageTransport};
    use fundamental_schema::SchemaNode;

    #[test]
    fn a_pubsub_message_has_no_reply_and_a_transport_label() {
        let dto = MessageDto::from(&MessageDescriptor::new(
            "events.user.created",
            MessageTransport::JetStream,
            "A user was created.",
            SchemaNode::Any,
        ));
        assert_eq!(dto.subject, "events.user.created");
        assert_eq!(dto.transport, "jetstream");
        assert_eq!(dto.reply, None);
    }

    #[test]
    fn an_rpc_carries_its_reply_and_the_rpc_label() {
        let dto = MessageDto::from(&MessageDescriptor::request_reply(
            "acme.audit.rpc",
            "The audit RPC.",
            SchemaNode::Bool,
            SchemaNode::Any,
            None,
        ));
        assert_eq!(dto.transport, "nats_rpc");
        assert_eq!(dto.reply, Some(SchemaDto::Any));

        let json = serde_json::to_string(&dto).expect("serialize");
        assert_eq!(serde_json::from_str::<MessageDto>(&json).unwrap(), dto);
    }
}
