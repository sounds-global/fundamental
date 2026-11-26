//! [`IssuedCertificate`] — what a PKI role signed.

use serde_json::Value;

use crate::error::SecretError;

/// A certificate custody issued through a PKI role, with its chain, in PEM exactly as returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCertificate {
    /// The issued certificate.
    pub certificate_pem: String,
    /// The certificate of the CA that issued it.
    pub issuing_ca_pem: String,
    /// The chain from the issuing CA upward, as custody lists it.
    pub ca_chain_pem: Vec<String>,
    /// The issued certificate's serial number, as custody spells it.
    pub serial_number: String,
    /// When the certificate stops being valid, in seconds since the Unix epoch.
    pub expiration: i64,
}

impl IssuedCertificate {
    /// Reads the `data` block of a `…/sign/<role>` response.
    ///
    /// # Errors
    /// [`SecretError::UnexpectedResponse`] when a required field is absent or not a string.
    pub(crate) fn from_response(response: &Value, path: &str) -> Result<Self, SecretError> {
        let text = |field: &str| {
            response
                .pointer(&format!("/data/{field}"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| SecretError::UnexpectedResponse {
                    path: path.to_owned(),
                    message: format!("the signed certificate carries no `{field}`"),
                })
        };
        let ca_chain_pem = match response.pointer("/data/ca_chain") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(chain)) => chain
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| {
                        SecretError::UnexpectedResponse {
                            path: path.to_owned(),
                            message: "`ca_chain` holds something other than PEM text".to_owned(),
                        }
                    })
                })
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(SecretError::UnexpectedResponse {
                    path: path.to_owned(),
                    message: "`ca_chain` is not a list".to_owned(),
                });
            }
        };
        let expiration = response
            .pointer("/data/expiration")
            .and_then(Value::as_i64)
            .ok_or_else(|| SecretError::UnexpectedResponse {
                path: path.to_owned(),
                message: "the signed certificate carries no `expiration`".to_owned(),
            })?;
        Ok(Self {
            certificate_pem: text("certificate")?,
            issuing_ca_pem: text("issuing_ca")?,
            ca_chain_pem,
            serial_number: text("serial_number")?,
            expiration,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_signed_response_is_read_whole() {
        let response = json!({ "data": {
            "certificate": "-----BEGIN CERTIFICATE-----\nleaf\n-----END CERTIFICATE-----",
            "issuing_ca": "-----BEGIN CERTIFICATE-----\nca\n-----END CERTIFICATE-----",
            "ca_chain": ["-----BEGIN CERTIFICATE-----\nca\n-----END CERTIFICATE-----"],
            "serial_number": "3a:1f",
            "expiration": 1_790_000_000,
        }});
        let issued = IssuedCertificate::from_response(&response, "pki/sign/r").expect("whole");
        assert!(issued.certificate_pem.contains("leaf"));
        assert_eq!(issued.ca_chain_pem.len(), 1);
        assert_eq!(issued.serial_number, "3a:1f");
        assert_eq!(issued.expiration, 1_790_000_000);
    }

    #[test]
    fn a_response_missing_its_certificate_is_refused() {
        let response = json!({ "data": { "issuing_ca": "x", "serial_number": "1" } });
        assert!(matches!(
            IssuedCertificate::from_response(&response, "pki/sign/r"),
            Err(SecretError::UnexpectedResponse { .. })
        ));
    }

    #[test]
    fn a_chain_that_is_not_text_is_refused() {
        let response = json!({ "data": {
            "certificate": "c", "issuing_ca": "i", "serial_number": "1", "expiration": 1, "ca_chain": [1]
        }});
        assert!(IssuedCertificate::from_response(&response, "pki/sign/r").is_err());
    }
}
