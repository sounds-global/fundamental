use serde::Serialize;
use serde_json::{Map, Value, json};

use fundamental_nats::MessageDescriptor;

/// An [AsyncAPI 3.0] description of a messaging surface.
///
/// [AsyncAPI 3.0]: https://www.asyncapi.com/docs/reference/specification/v3.0.0
#[derive(Debug, Clone)]
pub struct AsyncApiDocument {
    title: String,
    version: String,
    messages: Vec<MessageDescriptor>,
}

impl AsyncApiDocument {
    /// Builds a document from a title, an API version, and the message descriptors.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        version: impl Into<String>,
        messages: Vec<MessageDescriptor>,
    ) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            messages,
        }
    }

    /// Renders the document as a JSON value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut channels: Map<String, Value> = Map::new();
        let mut operations: Map<String, Value> = Map::new();

        for (index, message) in self.messages.iter().enumerate() {
            let channel_id = format!("channel{index}");
            let message_id = "payload";

            let mut message_object = Map::new();
            if !message.doc.is_empty() {
                message_object.insert("summary".into(), Value::String(message.doc.to_string()));
            }
            message_object.insert("payload".into(), json!(message.payload));

            channels.insert(
                channel_id.clone(),
                json!({
                    "address": message.subject,
                    // Request/reply is labelled apart from one-way pub/sub on the same wire.
                    "x-transport": message.transport_label(),
                    "messages": { message_id: Value::Object(message_object) }
                }),
            );

            let mut operation = json!({
                "action": "receive",
                "channel": { "$ref": format!("#/channels/{channel_id}") }
            });

            // A `#[respond]` RPC replies on the caller's inbox: a second channel with a `null` address.
            if let Some(reply) = &message.reply {
                let reply_channel_id = format!("{channel_id}Reply");
                let reply_message_id = "reply";
                channels.insert(
                    reply_channel_id.clone(),
                    json!({
                        "address": Value::Null,
                        "x-transport": message.transport_label(),
                        "messages": { reply_message_id: { "payload": json!(reply) } }
                    }),
                );
                operation["reply"] = json!({
                    "channel": { "$ref": format!("#/channels/{reply_channel_id}") },
                    "messages": [
                        { "$ref": format!("#/channels/{reply_channel_id}/messages/{reply_message_id}") }
                    ]
                });
            }

            operations.insert(format!("receive{index}"), operation);
        }

        json!({
            "asyncapi": "3.0.0",
            "info": { "title": self.title, "version": self.version },
            "channels": Value::Object(channels),
            "operations": Value::Object(operations),
        })
    }
}

impl Serialize for AsyncApiDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_nats::MessageTransport;
    use fundamental_schema::SchemaNode;

    fn user_created() -> MessageDescriptor {
        MessageDescriptor::new(
            "events.user.created",
            MessageTransport::Nats,
            "A user was created.",
            SchemaNode::Object {
                properties: vec![(
                    "user_id".into(),
                    SchemaNode::String {
                        format: Some("uuid"),
                        pattern: None,
                    },
                )],
                required: vec!["user_id".into()],
            },
        )
    }

    #[test]
    fn renders_a_versioned_document() {
        let doc = AsyncApiDocument::new("Demo", "1.0.0", vec![user_created()]).to_value();
        assert_eq!(doc["asyncapi"], "3.0.0");
        assert_eq!(doc["info"]["title"], "Demo");
    }

    #[test]
    fn a_subscription_becomes_a_channel_with_a_receive_operation() {
        let doc = AsyncApiDocument::new("Demo", "1", vec![user_created()]).to_value();
        assert_eq!(
            doc["channels"]["channel0"]["address"],
            "events.user.created"
        );
        assert_eq!(doc["channels"]["channel0"]["x-transport"], "nats");
        assert_eq!(
            doc["channels"]["channel0"]["messages"]["payload"]["payload"]["properties"]["user_id"]
                ["format"],
            "uuid"
        );
        assert_eq!(doc["operations"]["receive0"]["action"], "receive");
        assert_eq!(
            doc["operations"]["receive0"]["channel"]["$ref"],
            "#/channels/channel0"
        );
    }

    #[test]
    fn empty_surface_still_renders() {
        let doc = AsyncApiDocument::new("Demo", "1", vec![]).to_value();
        assert!(doc["channels"].as_object().unwrap().is_empty());
    }

    fn audit_rpc() -> MessageDescriptor {
        MessageDescriptor::request_reply(
            "acme.audit.rpc",
            "The audit RPC.",
            SchemaNode::Bool,
            SchemaNode::String {
                format: None,
                pattern: None,
            },
            None,
        )
    }

    #[test]
    fn a_request_reply_rpc_renders_a_reply_channel_and_operation_reply() {
        let doc = AsyncApiDocument::new("Demo", "1", vec![audit_rpc()]).to_value();

        assert_eq!(doc["channels"]["channel0"]["address"], "acme.audit.rpc");
        assert_eq!(doc["channels"]["channel0"]["x-transport"], "nats_rpc");
        assert_eq!(
            doc["channels"]["channel0"]["messages"]["payload"]["payload"]["type"],
            "boolean"
        );

        assert_eq!(doc["channels"]["channel0Reply"]["address"], Value::Null);
        assert_eq!(
            doc["channels"]["channel0Reply"]["messages"]["reply"]["payload"]["type"],
            "string"
        );

        // The operation is a `receive` with an AsyncAPI 3.0 Reply Object pointing at it.
        assert_eq!(doc["operations"]["receive0"]["action"], "receive");
        assert_eq!(
            doc["operations"]["receive0"]["reply"]["channel"]["$ref"],
            "#/channels/channel0Reply"
        );
        assert_eq!(
            doc["operations"]["receive0"]["reply"]["messages"][0]["$ref"],
            "#/channels/channel0Reply/messages/reply"
        );
    }

    #[test]
    fn a_mixed_surface_keeps_pubsub_reply_free() {
        let doc = AsyncApiDocument::new("Demo", "1", vec![user_created(), audit_rpc()]).to_value();

        assert_eq!(doc["channels"]["channel0"]["x-transport"], "nats");
        assert!(doc["channels"].get("channel0Reply").is_none());
        assert!(doc["operations"]["receive0"].get("reply").is_none());

        assert_eq!(doc["channels"]["channel1"]["x-transport"], "nats_rpc");
        assert!(doc["channels"]["channel1Reply"].is_object());
        assert_eq!(
            doc["operations"]["receive1"]["reply"]["channel"]["$ref"],
            "#/channels/channel1Reply"
        );
    }
}
