use serde::Serialize;
use serde::de::DeserializeOwned;

/// Encodes a carrier to its column's JSON; a failure, never seen in practice, gives an empty
/// string rather than a panic in generated code.
#[doc(hidden)]
pub fn encode_envelope<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// Decodes a column value written by [`encode_envelope`].
#[doc(hidden)]
pub fn decode_envelope<T: DeserializeOwned>(stored: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(stored)
}
