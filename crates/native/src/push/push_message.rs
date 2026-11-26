use fundamental_core::error::PayloadRejection;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{NativeError, PushDelivery};

/// An inbound push notification: how it arrived, and what it carried.
///
/// The payload is always a JSON object, checked at the edge, and never logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushMessage {
    delivery: PushDelivery,
    data: Value,
}

impl PushMessage {
    /// Builds a message from an already-parsed payload.
    ///
    /// # Errors
    /// [`NativeError::PushPayload`] when `data` is not a JSON object.
    pub fn new(delivery: PushDelivery, data: Value) -> Result<Self, NativeError> {
        if !data.is_object() {
            return Err(NativeError::PushPayload {
                detail: "the payload is not a JSON object".to_owned(),
            });
        }
        Ok(Self { delivery, data })
    }

    /// Builds a message from the platform's raw JSON text.
    ///
    /// # Errors
    /// [`NativeError::PushPayload`] when the text is not a JSON object.
    pub fn from_json(delivery: PushDelivery, data: &str) -> Result<Self, NativeError> {
        let value = serde_json::from_str(data).map_err(|error| NativeError::PushPayload {
            detail: PayloadRejection::of(&error).to_string(),
        })?;
        Self::new(delivery, value)
    }

    /// How the notification reached the app.
    #[must_use]
    pub fn delivery(&self) -> &PushDelivery {
        &self.delivery
    }

    /// The payload, guaranteed to be a JSON object.
    #[must_use]
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// Deserializes the payload into the app's own type.
    ///
    /// # Errors
    /// [`NativeError::PushPayload`] when the payload does not match `T`; the message names the
    /// position, never the value.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T, NativeError> {
        serde_json::from_value(self.data.clone()).map_err(|error| NativeError::PushPayload {
            detail: PayloadRejection::of(&error).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use serde::Deserialize;
    use serde_json::json;

    use super::PushMessage;
    use crate::{NativeError, PushDelivery};

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Update {
        request_id: String,
    }

    #[test]
    fn an_object_payload_round_trips_to_a_typed_value() {
        let message = PushMessage::from_json(PushDelivery::Background, r#"{"request_id":"req-7"}"#)
            .expect("an object payload is valid");
        assert_eq!(
            message.parse::<Update>().expect("the shape matches"),
            Update {
                request_id: "req-7".to_owned(),
            }
        );
        assert_eq!(message.delivery(), &PushDelivery::Background);
    }

    #[test]
    fn a_non_object_payload_is_refused_at_construction() {
        let err = PushMessage::new(PushDelivery::Foreground, json!(42))
            .expect_err("a bare scalar is not a notification payload");
        assert!(matches!(err, NativeError::PushPayload { .. }));
        assert_eq!(err.error_code(), "NATIVE_023");
    }

    #[test]
    fn malformed_json_is_refused_at_construction() {
        let err = PushMessage::from_json(PushDelivery::Foreground, "{oops")
            .expect_err("malformed text is not a payload");
        assert!(format!("{err}").contains("the input is not JSON"), "{err}");
    }

    #[test]
    fn a_shape_mismatch_never_names_the_value() {
        let message = PushMessage::from_json(
            PushDelivery::Foreground,
            r#"{"request_id":"nachtwacht-1642"}"#,
        )
        .expect("an object payload is valid");

        #[derive(Debug, Deserialize)]
        struct Numeric {
            #[expect(
                dead_code,
                reason = "read by serde only: the test is its decode failing"
            )]
            request_id: u64,
        }

        let err = message
            .parse::<Numeric>()
            .expect_err("a string is not a u64");
        let rendered = format!("{err}");
        assert!(
            !rendered.contains("nachtwacht-1642"),
            "a payload value must not reach the error text: {rendered}"
        );
        assert!(rendered.contains("of the wrong type"), "{rendered}");
    }
}
