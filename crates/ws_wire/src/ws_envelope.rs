//! [`WsEnvelope`] — the wire form of a WebSocket message.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The JSON envelope every frame carries: an `action` naming the handler and its `data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEnvelope {
    /// The action string used to route the message to a handler.
    pub action: String,
    /// The payload, deserialized into the handler's type at dispatch.
    #[serde(default)]
    pub data: Value,
}

impl WsEnvelope {
    /// Builds an envelope for `action` carrying `data`.
    pub fn new(action: impl Into<String>, data: Value) -> Self {
        Self {
            action: action.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_action_and_data() {
        let envelope: WsEnvelope =
            serde_json::from_str(r#"{"action":"chat","data":{"text":"hi"}}"#).expect("valid");
        assert_eq!(envelope.action, "chat");
        assert_eq!(envelope.data["text"], "hi");
    }

    #[test]
    fn data_defaults_to_null_when_absent() {
        let envelope: WsEnvelope = serde_json::from_str(r#"{"action":"ping"}"#).expect("valid");
        assert_eq!(envelope.action, "ping");
        assert!(envelope.data.is_null());
    }

    #[test]
    fn new_round_trips_through_json() {
        let envelope = WsEnvelope::new("slice.pull", serde_json::json!({ "since": 7 }));
        let text = serde_json::to_string(&envelope).expect("serialize");
        let parsed: WsEnvelope = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed.action, "slice.pull");
        assert_eq!(parsed.data["since"], 7);
    }
}
