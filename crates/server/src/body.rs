use fundamental_core::AnyError;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ServerError;

/// Decodes a request or response body as JSON into `T`; an empty body is [`None`].
///
/// # Errors
/// [`ServerError::Deserialize`] (`API_007`, status `400`) if a non-empty `body` is
/// not valid JSON for `T`.
pub fn decode_body<T: DeserializeOwned>(body: &[u8]) -> Result<Option<T>, AnyError> {
    if body.is_empty() {
        return Ok(None);
    }
    decode_json(body).map(Some)
}

/// Encodes `value` as a JSON body.
///
/// # Errors
/// [`ServerError::Serialize`] (`API_008`, status `500`) if `value` cannot be
/// serialized.
pub fn encode_body<T: Serialize>(value: &T) -> Result<Vec<u8>, AnyError> {
    serde_json::to_vec(value)
        .map_err(|error| AnyError::new(ServerError::serialize(error.to_string())))
}

/// Deserializes `bytes` as JSON into `T`, mapping any failure to [`ServerError::Deserialize`].
pub(crate) fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AnyError> {
    serde_json::from_slice(bytes)
        .map_err(|error| AnyError::new(ServerError::deserialize(error.to_string())))
}

/// Deserializes a URL query string into `T`, mapping any failure to [`ServerError::Deserialize`].
pub(crate) fn decode_query<T: DeserializeOwned>(query: &str) -> Result<T, AnyError> {
    serde_urlencoded::from_str(query)
        .map_err(|error| AnyError::new(ServerError::deserialize(error.to_string())))
}

/// Deserializes an urlencoded form body into `T`, mapping any failure to [`ServerError::Deserialize`].
pub(crate) fn decode_form<T: DeserializeOwned>(body: &[u8]) -> Result<T, AnyError> {
    serde_urlencoded::from_bytes(body)
        .map_err(|error| AnyError::new(ServerError::deserialize(error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Payload {
        n: u64,
    }

    #[test]
    fn decodes_a_form_urlencoded_body() {
        let decoded: Payload = decode_form(b"n=7").expect("decodes form body");
        assert_eq!(decoded, Payload { n: 7 });
    }

    #[test]
    fn malformed_form_body_is_a_deserialize_error() {
        let err = decode_form::<Payload>(b"n=oops").expect_err("bad form");
        assert_eq!(err.get().error_code(), "API_007");
        assert_eq!(err.get().http_status_code(), 400);
    }

    #[test]
    fn decodes_a_present_body_and_round_trips() {
        let bytes = encode_body(&Payload { n: 7 }).expect("encodes");
        let decoded: Option<Payload> = decode_body(&bytes).expect("decodes");
        assert_eq!(decoded, Some(Payload { n: 7 }));
    }

    #[test]
    fn empty_body_decodes_to_none() {
        let decoded: Option<Payload> = decode_body(&[]).expect("empty is ok");
        assert_eq!(decoded, None);
    }

    #[test]
    fn malformed_body_is_a_deserialize_error() {
        let err = decode_body::<Payload>(br#"{"n":"oops"}"#).expect_err("bad json");
        assert_eq!(err.get().error_code(), "API_007");
        assert_eq!(err.get().http_status_code(), 400);
    }
}
