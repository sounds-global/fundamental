use fundamental_schema::SchemaNode;

/// The transport a message travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageTransport {
    /// Core NATS pub/sub.
    Nats,
    /// A durable NATS JetStream consumer.
    JetStream,
    /// An action-routed WebSocket message.
    Socket,
}

impl MessageTransport {
    /// A stable, lowercase label for the transport (`"nats"`, `"jetstream"`, `"socket"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MessageTransport::Nats => "nats",
            MessageTransport::JetStream => "jetstream",
            MessageTransport::Socket => "socket",
        }
    }
}

/// A machine-readable description of a message: its subject, transport and payload.
///
/// Built by `mount_messages!` and `#[respond]`, so the messaging surface is data.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MessageDescriptor {
    /// The subject the message is published to (`"__dynamic__"` for a per-value subject).
    pub subject: &'static str,
    /// The transport it travels on.
    pub transport: MessageTransport,
    /// The message type's documentation (empty when undocumented).
    pub doc: &'static str,
    /// The payload's shape, the request's for an RPC; [`SchemaNode::Any`] without a schema.
    pub payload: SchemaNode,
    /// The reply payload's shape for a request/reply RPC; `None` for one-way pub/sub.
    pub reply: Option<SchemaNode>,
    /// The queue group a `#[respond]` responder joins, or `None`; it decides who receives a
    /// message and is what a broker allow-list pins.
    pub queue_group: Option<&'static str>,
}

impl MessageDescriptor {
    /// Builds a one-way pub/sub descriptor.
    #[must_use]
    pub fn new(
        subject: &'static str,
        transport: MessageTransport,
        doc: &'static str,
        payload: SchemaNode,
    ) -> Self {
        Self {
            subject,
            transport,
            doc,
            payload,
            reply: None,
            queue_group: None,
        }
    }

    /// Builds a request/reply RPC descriptor, with the group the responder joins.
    #[must_use]
    pub fn request_reply(
        subject: &'static str,
        doc: &'static str,
        request: SchemaNode,
        reply: SchemaNode,
        queue_group: Option<&'static str>,
    ) -> Self {
        Self {
            subject,
            transport: MessageTransport::Nats,
            doc,
            payload: request,
            reply: Some(reply),
            queue_group,
        }
    }

    /// Whether this describes a request/reply RPC (as opposed to one-way pub/sub).
    #[must_use]
    pub fn is_request_reply(&self) -> bool {
        self.reply.is_some()
    }

    /// The display label for the transport, `"nats_rpc"` for a request/reply RPC.
    #[must_use]
    pub fn transport_label(&self) -> &'static str {
        if self.is_request_reply() {
            "nats_rpc"
        } else {
            self.transport.as_str()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_labels_are_stable() {
        assert_eq!(MessageTransport::Nats.as_str(), "nats");
        assert_eq!(MessageTransport::JetStream.as_str(), "jetstream");
        assert_eq!(MessageTransport::Socket.as_str(), "socket");
    }

    #[test]
    fn descriptor_carries_its_fields() {
        let descriptor = MessageDescriptor::new(
            "events.user.created",
            MessageTransport::Nats,
            "A user was created.",
            SchemaNode::Any,
        );
        assert_eq!(descriptor.subject, "events.user.created");
        assert_eq!(descriptor.transport, MessageTransport::Nats);
    }

    #[test]
    fn a_pubsub_descriptor_has_no_reply() {
        let descriptor = MessageDescriptor::new(
            "events.user.created",
            MessageTransport::Nats,
            "",
            SchemaNode::Any,
        );
        assert!(!descriptor.is_request_reply());
        assert_eq!(descriptor.reply, None);
        assert_eq!(descriptor.transport_label(), "nats");
    }

    #[test]
    fn a_request_reply_descriptor_carries_both_schemas() {
        let descriptor = MessageDescriptor::request_reply(
            "acme.audit.rpc",
            "The audit RPC.",
            SchemaNode::Bool,
            SchemaNode::Any,
            Some("layer5_log"),
        );
        assert!(descriptor.is_request_reply());
        assert_eq!(descriptor.payload, SchemaNode::Bool);
        assert_eq!(descriptor.reply, Some(SchemaNode::Any));
        assert_eq!(descriptor.transport, MessageTransport::Nats);
        assert_eq!(descriptor.transport_label(), "nats_rpc");
        assert_eq!(descriptor.queue_group, Some("layer5_log"));
    }

    /// A one-way descriptor has no group.
    #[test]
    fn a_one_way_descriptor_names_no_queue_group() {
        let descriptor = MessageDescriptor::new(
            "events.layer1.credential.changed",
            MessageTransport::Nats,
            "",
            SchemaNode::Any,
        );
        assert_eq!(descriptor.queue_group, None);
    }
}
