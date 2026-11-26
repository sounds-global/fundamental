use std::str::FromStr as _;

use const_oid::ObjectIdentifier;
use const_oid::db::rfc5912::ECDSA_WITH_SHA_256;
use der::asn1::{Any, BitString, SetOfVec, Utf8StringRef};
use der::{Decode as _, Encode as _, EncodePem as _};
use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use tracing::instrument;
use x509_cert::attr::{Attribute, Attributes};
use x509_cert::name::Name;
use x509_cert::request::{CertReq, CertReqInfo, Version};

use super::DeviceKey;
use crate::KeyError;

/// The PKCS #9 `challengePassword` attribute (RFC 2985 §5.4.1).
const CHALLENGE_PASSWORD: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.7");

/// A PEM PKCS #10 certificate request (RFC 2986) for `key`, signed by `key`, naming
/// `common_name` and carrying `challenge` as its `challengePassword` when given.
///
/// # Errors
/// [`KeyError::Malformed`] when the name or the key does not encode, and the key's own
/// [`KeyError::Sign`] when it will not sign.
#[instrument(skip(key, challenge), fields(challenge = challenge.is_some()), err(Debug))]
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub fn certificate_request(
    key: &dyn DeviceKey,
    common_name: &str,
    challenge: Option<&str>,
) -> Result<String, KeyError> {
    let malformed = |error: &dyn std::fmt::Display| KeyError::Malformed(error.to_string());
    let info = CertReqInfo {
        version: Version::V1,
        subject: Name::from_str(&format!("CN={common_name}")).map_err(|error| malformed(&error))?,
        public_key: SubjectPublicKeyInfoOwned::from_der(key.public_key_der())
            .map_err(|error| malformed(&error))?,
        attributes: attributes(challenge).map_err(|error| malformed(&error))?,
    };
    let signature = key.sign(&info.to_der().map_err(|error| malformed(&error))?)?;
    CertReq {
        info,
        algorithm: AlgorithmIdentifierOwned {
            oid: ECDSA_WITH_SHA_256,
            parameters: None,
        },
        signature: BitString::from_bytes(&signature).map_err(|error| malformed(&error))?,
    }
    .to_pem(der::pem::LineEnding::LF)
    .map_err(|error| malformed(&error))
}

fn attributes(challenge: Option<&str>) -> Result<Attributes, der::Error> {
    let mut attributes = Attributes::new();
    if let Some(challenge) = challenge {
        attributes.insert(Attribute {
            oid: CHALLENGE_PASSWORD,
            values: SetOfVec::try_from(vec![Any::encode_from(&Utf8StringRef::new(challenge)?)?])?,
        })?;
    }
    Ok(attributes)
}

#[cfg(test)]
mod tests {
    use der::DecodePem as _;
    use p256::ecdsa::signature::Verifier as _;
    use p256::ecdsa::{DerSignature, VerifyingKey};
    use p256::pkcs8::DecodePublicKey as _;
    use x509_cert::request::CertReq;

    use super::{CHALLENGE_PASSWORD, certificate_request};
    use crate::{DeviceKey, MemorySecretStore, SealedDeviceKey};

    #[test]
    fn the_request_names_the_key_and_the_key_signed_it() {
        let store = MemorySecretStore::new();
        let key = SealedDeviceKey::open_or_create(&store, "computer").expect("a key");
        let pem = certificate_request(&key, "Acme operations", Some("nonce-1")).expect("a request");
        let request = CertReq::from_pem(pem.as_bytes()).expect("PKCS #10");
        let public = der::Encode::to_der(&request.info.public_key).expect("SPKI");
        assert_eq!(public, key.public_key_der());
        let verifying = VerifyingKey::from_public_key_der(&public).expect("P-256");
        let signed = der::Encode::to_der(&request.info).expect("info");
        let signature = DerSignature::from_bytes(request.signature.raw_bytes()).expect("DER");
        verifying
            .verify(&signed, &signature)
            .expect("the key signed what the request says");
        assert!(
            request
                .info
                .attributes
                .iter()
                .any(|attribute| attribute.oid == CHALLENGE_PASSWORD),
            "the nonce rides inside what the key signed"
        );
    }

    #[test]
    fn a_request_without_a_challenge_carries_none() {
        let store = MemorySecretStore::new();
        let key = SealedDeviceKey::open_or_create(&store, "computer").expect("a key");
        let pem = certificate_request(&key, "Acme operations", None).expect("a request");
        let request = CertReq::from_pem(pem.as_bytes()).expect("PKCS #10");
        assert!(request.info.attributes.is_empty());
    }
}
