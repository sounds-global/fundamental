//! [`TransitKeyInfo`] — what a Transit engine reports about a key.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::SecretError;

/// The fields of a Transit key read that decide whether the key may back a signer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitKeyInfo {
    /// The engine's type string, verbatim so an unknown type is reported rather than coerced.
    pub key_type: String,
    /// Whether the key may be exported.
    pub exportable: bool,
    /// Whether the key may be backed up in plaintext.
    pub allow_plaintext_backup: bool,
    /// The newest key version.
    pub latest_version: u32,
    /// The public key of each version, as the engine encodes it.
    pub public_keys: BTreeMap<u32, String>,
}

impl TransitKeyInfo {
    /// Reads a key-read or key-create response body.
    ///
    /// # Errors
    /// [`SecretError::UnexpectedResponse`] if a field this client relies on is missing.
    pub fn from_response(response: &Value, path: &str) -> Result<Self, SecretError> {
        let missing = |field: &str| SecretError::UnexpectedResponse {
            path: path.to_owned(),
            message: format!("missing data.{field}"),
        };
        let data = response.get("data").ok_or_else(|| missing("data"))?;
        let key_type = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("type"))?
            .to_owned();
        let exportable = data
            .get("exportable")
            .and_then(Value::as_bool)
            .ok_or_else(|| missing("exportable"))?;
        let allow_plaintext_backup = data
            .get("allow_plaintext_backup")
            .and_then(Value::as_bool)
            .ok_or_else(|| missing("allow_plaintext_backup"))?;
        let latest_version = data
            .get("latest_version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| missing("latest_version"))?;
        let keys = data
            .get("keys")
            .and_then(Value::as_object)
            .ok_or_else(|| missing("keys"))?;
        let mut public_keys = BTreeMap::new();
        for (version, entry) in keys {
            let Ok(version) = version.parse::<u32>() else {
                continue;
            };
            if let Some(public_key) = entry.get("public_key").and_then(Value::as_str) {
                public_keys.insert(version, public_key.to_owned());
            }
        }
        Ok(Self {
            key_type,
            exportable,
            allow_plaintext_backup,
            latest_version,
            public_keys,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape OpenBao 2.6.2 returned for `POST transit/keys/{name}`.
    fn recorded() -> Value {
        json!({"data":{"allow_plaintext_backup":false,"exportable":false,"keys":{"1":{
            "name":"P-256","public_key":"-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----\n"}},
            "latest_version":1,"type":"ecdsa-p256"}})
    }

    #[test]
    fn reads_a_recorded_response() {
        let info = TransitKeyInfo::from_response(&recorded(), "transit/keys/t").expect("parses");
        assert_eq!(info.key_type, "ecdsa-p256");
        assert!(!info.exportable);
        assert_eq!(info.latest_version, 1);
        assert!(info.public_keys[&1].starts_with("-----BEGIN PUBLIC KEY-----"));
    }

    #[test]
    fn a_missing_field_is_reported_not_defaulted() {
        let mut body = recorded();
        body["data"]
            .as_object_mut()
            .expect("object")
            .remove("exportable");
        assert!(matches!(
            TransitKeyInfo::from_response(&body, "transit/keys/t"),
            Err(SecretError::UnexpectedResponse { .. })
        ));
    }
}
